const vscode = require("vscode");
const net = require("net");
const fs = require("fs");
const path = require("path");
const crypto = require("crypto");
const { spawn } = require("child_process");
const {
  OP,
  MAX_PACKET_BYTES,
  buildFramedPacket,
  encodeBufferUpdatePayload,
  forwardWebviewCommand,
  terminateChildProcessTree,
} = require("./panel_protocol");

let activeSession = null;

function activate(context) {
  const diagnostics = vscode.languages.createDiagnosticCollection("krui");
  const output = vscode.window.createOutputChannel("KRui Live Preview");
  context.subscriptions.push(diagnostics, output);

  const openCmd = vscode.commands.registerCommand("krui.openLivePreview", () => {
    const editor = vscode.window.activeTextEditor;
    if (!editor) {
      vscode.window.showWarningMessage("Open a .krui file first to launch KRui Live Preview.");
      return;
    }
    const targetPath = editor.document.fileName;
    if (!isLiveSourceFile(targetPath)) {
      vscode.window.showWarningMessage("KRui Live Preview supports .krui and .css files.");
      return;
    }
    if (activeSession && activeSession.targetPath === targetPath && !activeSession.isExited()) {
      activeSession.panel.reveal(vscode.ViewColumn.Beside);
      return;
    }
    if (activeSession) {
      activeSession.dispose();
      activeSession = null;
    }
    activeSession = createPreviewSession(context, diagnostics, output, targetPath);
  });

  const changeSub = vscode.workspace.onDidChangeTextDocument((e) => {
    if (!activeSession || e.document.uri.scheme !== "file") return;
    if (isLiveSourceFile(e.document.fileName)) {
      activeSession.sendBufferUpdate(e.document.fileName, e.document.getText());
    }
  });

  const saveSub = vscode.workspace.onDidSaveTextDocument((doc) => {
    if (!activeSession || doc.uri.scheme !== "file") return;
    if (isLiveSourceFile(doc.fileName)) {
      activeSession.sendBufferSaved(doc.fileName);
    }
  });

  context.subscriptions.push(openCmd, changeSub, saveSub);
}

function deactivate() {
  if (activeSession) {
    activeSession.dispose();
    activeSession = null;
  }
}

function isLiveSourceFile(filePath) {
  return filePath.endsWith(".krui") || filePath.endsWith(".css");
}

function findWorkspaceRoot(filePath) {
  const folder = vscode.workspace.getWorkspaceFolder(vscode.Uri.file(filePath));
  if (folder && fs.existsSync(path.join(folder.uri.fsPath, "Cargo.toml"))) {
    return folder.uri.fsPath;
  }
  let dir = path.dirname(filePath);
  for (let i = 0; i < 8; i++) {
    if (fs.existsSync(path.join(dir, "crates", "cli", "Cargo.toml"))) {
      return dir;
    }
    const parent = path.dirname(dir);
    if (parent === dir) break;
    dir = parent;
  }
  return folder ? folder.uri.fsPath : path.dirname(filePath);
}

function buildCliCommand(workspaceRoot, serveArgs) {
  const config = vscode.workspace.getConfiguration("krui");
  const cliPath = String(config.get("cliPath") || "").trim();
  if (cliPath.length > 0) {
    return { cmd: cliPath, args: serveArgs };
  }
  const binName = process.platform === "win32" ? "krui.exe" : "krui";
  const prebuilt = path.join(workspaceRoot, "target", "debug", binName);
  if (fs.existsSync(prebuilt)) {
    return { cmd: prebuilt, args: serveArgs };
  }
  const hasWorkspaceCli = fs.existsSync(path.join(workspaceRoot, "crates", "cli", "Cargo.toml"));
  if (hasWorkspaceCli) {
    return { cmd: "cargo", args: ["run", "-q", "-p", "krui-cli", "--", ...serveArgs] };
  }
  return { cmd: "krui", args: serveArgs };
}

function createWebviewPanel(context, targetPath, defaultTheme, defaultFrame) {
  const panel = vscode.window.createWebviewPanel(
    "kruiLivePreview",
    `KRui Preview: ${path.basename(targetPath)}`,
    vscode.ViewColumn.Beside,
    {
      enableScripts: true,
      retainContextWhenHidden: true,
      localResourceRoots: [vscode.Uri.file(path.join(context.extensionPath, "media"))],
    }
  );
  panel.webview.html = renderWebviewHtml(context, panel.webview, defaultTheme, defaultFrame);
  return panel;
}

function attachChildStreams(ctx) {
  let stdoutBuf = "";
  let stderrTail = "";
  ctx.child.stdout.on("data", (chunk) => {
    const text = chunk.toString("utf8");
    ctx.output.append(text);
    stdoutBuf += text;
    const lines = stdoutBuf.split(/\r?\n/);
    stdoutBuf = lines.pop() || "";
    for (const line of lines) {
      const m = line.match(/PANEL_ADDR=([^\s]+)/);
      if (m) connectBridgeSocket(ctx, m[1]);
    }
  });
  ctx.child.stderr.on("data", (chunk) => {
    const text = chunk.toString("utf8");
    ctx.output.append(text);
    stderrTail = (stderrTail + text).slice(-4000);
  });
  ctx.child.on("error", (err) => {
    ctx.childExited = true;
    ctx.output.appendLine(`[spawn error] ${err.message}`);
    ctx.postToWebview({
      kind: "compileError",
      file: ctx.targetPath,
      line: 1,
      column: 1,
      message: `Failed to start krui CLI: ${err.message}`,
    });
  });
  ctx.child.on("exit", (code) => {
    ctx.childExited = true;
    ctx.output.appendLine(`[exit] krui serve exited with code ${code}`);
    if (!ctx.disposed && code !== 0) {
      ctx.postToWebview({
        kind: "compileError",
        file: ctx.targetPath,
        line: 1,
        column: 1,
        message: stderrTail.trim() || `krui serve exited with code ${code}`,
      });
    }
  });
}

function handleServerJson(ctx, msg) {
  if (msg.kind === "compileError") {
    const absFile = ctx.resolveDocPath(msg.file);
    const line = Math.max(0, Number(msg.line || 1) - 1);
    const col = Math.max(0, Number(msg.column || 1) - 1);
    const range = new vscode.Range(line, col, line, col + 20);
    ctx.diagnostics.set(vscode.Uri.file(absFile), [
      new vscode.Diagnostic(
        range,
        String(msg.message || "Compile error"),
        vscode.DiagnosticSeverity.Error
      ),
    ]);
    ctx.postToWebview(msg);
    return;
  }
  if (msg.kind === "compileOk") {
    ctx.diagnostics.clear();
    ctx.postToWebview(msg);
    return;
  }
  if (msg.kind === "jumpToSource") {
    jumpToSource(ctx.resolveDocPath(msg.file), msg.line, msg.column);
    return;
  }
  ctx.postToWebview(msg);
}

function drainIncomingFrames(ctx, sock) {
  while (ctx.rxBuf.length >= 4) {
    const bodyLen = ctx.rxBuf.readUInt32LE(0);
    if (bodyLen < 1 || bodyLen > MAX_PACKET_BYTES) {
      ctx.output.appendLine(`[panel] invalid frame length ${bodyLen}, closing`);
      sock.destroy();
      return;
    }
    if (ctx.rxBuf.length < 4 + bodyLen) break;
    const opcode = ctx.rxBuf[4];
    const payload = ctx.rxBuf.subarray(5, 4 + bodyLen);
    ctx.rxBuf = ctx.rxBuf.subarray(4 + bodyLen);
    if (opcode === 0x01) {
      ctx.postToWebview({ type: "krfr", data: payload.toString("base64") });
    } else if (opcode === 0x02) {
      try {
        handleServerJson(ctx, JSON.parse(payload.toString("utf8")));
      } catch (err) {
        ctx.output.appendLine(`[panel] JSON parse error: ${err}`);
      }
    }
  }
}

function connectBridgeSocket(ctx, hostPort) {
  if (ctx.disposed || ctx.socket) return;
  const lastColon = hostPort.lastIndexOf(":");
  const host = lastColon > 0 ? hostPort.slice(0, lastColon) : "127.0.0.1";
  const port = Number(lastColon > 0 ? hostPort.slice(lastColon + 1) : hostPort);
  if (!Number.isFinite(port) || port <= 0) return;

  ctx.output.appendLine(`[panel] connecting to ${host}:${port}`);
  const sock = net.createConnection({ host, port });
  sock.setNoDelay(true);
  ctx.socket = sock;

  sock.on("connect", () => {
    if (ctx.defaultTheme && ctx.defaultTheme !== "system") {
      ctx.sendPacket(OP.SET_COLOR_SCHEME, Buffer.from(ctx.defaultTheme, "utf8"));
    }
    for (const doc of vscode.workspace.textDocuments) {
      if (doc.uri.scheme === "file" && doc.isDirty && isLiveSourceFile(doc.fileName)) {
        ctx.sendBufferUpdate(doc.fileName, doc.getText());
      }
    }
  });
  sock.on("data", (chunk) => {
    ctx.rxBuf = Buffer.concat([ctx.rxBuf, chunk]);
    drainIncomingFrames(ctx, sock);
  });
  sock.on("error", (err) => {
    ctx.output.appendLine(`[panel] socket error: ${err.message}`);
  });
}

function createPreviewSession(context, diagnostics, output, targetPath) {
  const workspaceRoot = findWorkspaceRoot(targetPath);
  const config = vscode.workspace.getConfiguration("krui");
  const defaultTheme = String(config.get("defaultColorScheme") || "system");
  const defaultFrame = String(config.get("defaultDeviceFrame") || "none");
  const panel = createWebviewPanel(context, targetPath, defaultTheme, defaultFrame);

  const relWatch = path.relative(workspaceRoot, targetPath);
  const watchArg = relWatch && !relWatch.startsWith("..") ? relWatch : targetPath;
  const serveArgs = ["serve", "--panel", "--port", "0", "--watch", watchArg];
  if (defaultFrame && defaultFrame !== "none") serveArgs.push("--frame", defaultFrame);
  const { cmd, args } = buildCliCommand(workspaceRoot, serveArgs);
  output.appendLine(`[spawn] (${workspaceRoot}) ${cmd} ${args.join(" ")}`);

  const child = spawn(cmd, args, { cwd: workspaceRoot, shell: false, windowsHide: true });
  const ctx = buildSessionContext(panel, diagnostics, output, workspaceRoot, targetPath, defaultTheme, child);
  attachChildStreams(ctx);
  attachWebviewBridge(ctx);
  panel.onDidDispose(() => ctx.dispose());

  return {
    targetPath,
    panel,
    isExited: () => ctx.childExited,
    sendBufferUpdate: ctx.sendBufferUpdate,
    sendBufferSaved: ctx.sendBufferSaved,
    dispose: ctx.dispose,
  };
}

function buildSessionContext(panel, diagnostics, output, workspaceRoot, targetPath, defaultTheme, child) {
  const ctx = {
    panel,
    diagnostics,
    output,
    workspaceRoot,
    targetPath,
    defaultTheme,
    child,
    childExited: false,
    disposed: false,
    webviewReady: false,
    pendingToWebview: [],
    socket: null,
    rxBuf: Buffer.alloc(0),
    postToWebview(payload) {
      if (ctx.disposed) return;
      if (!ctx.webviewReady) {
        ctx.pendingToWebview.push(payload);
        if (ctx.pendingToWebview.length > 64) ctx.pendingToWebview.shift();
        return;
      }
      panel.webview.postMessage(payload);
    },
    sendPacket(opcode, body) {
      if (!ctx.socket || ctx.socket.destroyed || !ctx.socket.writable) return;
      ctx.socket.write(buildFramedPacket(opcode, body));
    },
    resolveDocPath(fileStr) {
      if (!fileStr) return targetPath;
      return path.isAbsolute(fileStr) ? fileStr : path.join(workspaceRoot, fileStr);
    },
    sendBufferUpdate(filePath, content) {
      if (!ctx.disposed) ctx.sendPacket(OP.BUFFER_UPDATE, encodeBufferUpdatePayload(filePath, content));
    },
    sendBufferSaved(filePath) {
      if (!ctx.disposed) ctx.sendPacket(OP.BUFFER_SAVED, Buffer.from(filePath, "utf8"));
    },
    dispose: () => disposeSession(ctx),
  };
  return ctx;
}

function attachWebviewBridge(ctx) {
  ctx.panel.webview.onDidReceiveMessage((msg) => {
    if (!msg || typeof msg !== "object") return;
    if (msg.type === "webviewReady") {
      ctx.webviewReady = true;
      for (const item of ctx.pendingToWebview) {
        ctx.panel.webview.postMessage(item);
      }
      ctx.pendingToWebview = [];
      return;
    }
    if (msg.type === "jumpToSource") {
      jumpToSource(ctx.resolveDocPath(msg.file), msg.line, msg.column);
      return;
    }
    forwardWebviewCommand(msg, ctx.sendPacket);
  });
}

function disposeSession(ctx) {
  if (ctx.disposed) return;
  ctx.disposed = true;
  ctx.diagnostics.clear();
  const sock = ctx.socket;
  if (sock && !sock.destroyed) {
    try {
      ctx.sendPacket(OP.SHUTDOWN, null);
      sock.end();
    } catch (_) {}
  }
  const killTimer = setTimeout(() => {
    if (sock && !sock.destroyed) {
      try {
        sock.destroy();
      } catch (_) {}
    }
    terminateChildProcessTree(ctx.child);
  }, 600);
  if (killTimer.unref) killTimer.unref();
  try {
    ctx.panel.dispose();
  } catch (_) {}
  if (activeSession && activeSession.targetPath === ctx.targetPath) {
    activeSession = null;
  }
}

async function jumpToSource(filePath, lineNum, colNum) {
  try {
    const uri = vscode.Uri.file(filePath);
    const doc = await vscode.workspace.openTextDocument(uri);
    const editor = await vscode.window.showTextDocument(doc, {
      viewColumn: vscode.ViewColumn.One,
      preserveFocus: false,
    });
    const line = Math.max(0, Number(lineNum || 1) - 1);
    const col = Math.max(0, Number(colNum || 1) - 1);
    const pos = new vscode.Position(line, col);
    editor.selection = new vscode.Selection(pos, pos);
    editor.revealRange(new vscode.Range(pos, pos), vscode.TextEditorRevealType.InCenterIfOutsideViewport);
  } catch (err) {
    vscode.window.showErrorMessage(`Failed to open ${filePath}: ${err.message}`);
  }
}

function renderWebviewHtml(context, webview, initialTheme, initialFrame) {
  const mediaRoot = path.join(context.extensionPath, "media");
  const htmlPath = path.join(mediaRoot, "webview.html");
  const cssUri = webview.asWebviewUri(vscode.Uri.file(path.join(mediaRoot, "webview.css")));
  const jsUri = webview.asWebviewUri(vscode.Uri.file(path.join(mediaRoot, "webview.js")));
  const nonce = crypto.randomBytes(16).toString("hex");
  return fs
    .readFileSync(htmlPath, "utf8")
    .replaceAll("{{cspSource}}", webview.cspSource)
    .replaceAll("{{nonce}}", nonce)
    .replaceAll("{{cssUri}}", String(cssUri))
    .replaceAll("{{jsUri}}", String(jsUri))
    .replaceAll("{{initialTheme}}", escapeHtmlAttr(initialTheme))
    .replaceAll("{{initialFrame}}", escapeHtmlAttr(initialFrame));
}

function escapeHtmlAttr(str) {
  return String(str).replace(/[^a-zA-Z0-9_-]/g, "");
}

module.exports = { activate, deactivate };

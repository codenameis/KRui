(function () {
  "use strict";

  const vscode = acquireVsCodeApi();
  const encoder = new TextEncoder();

  const toolbarPill = document.getElementById("renderer-pill");
  const themeSelect = document.getElementById("theme-select");
  const frameSelect = document.getElementById("frame-select");
  const ovRelayout = document.getElementById("ov-relayout");
  const ovFps = document.getElementById("ov-fps");
  const ovHit = document.getElementById("ov-hit");
  const btnInspect = document.getElementById("btn-inspect");
  const btnRestart = document.getElementById("btn-restart");
  const errorBanner = document.getElementById("banner-error");
  const stage = document.getElementById("stage");
  const canvas = document.getElementById("krui-canvas");
  const loadingOverlay = document.getElementById("loading-overlay");
  const loadingText = document.getElementById("loading-text");
  const imeBridge = document.getElementById("ime-bridge");
  const statSize = document.getElementById("stat-size");
  const statPerf = document.getElementById("stat-perf");
  const statNodes = document.getElementById("stat-nodes");
  const statReloads = document.getElementById("stat-reloads");
  const imeBadge = document.getElementById("ime-badge");

  const ctx = canvas.getContext("2d", { alpha: true });

  const state = {
    inspectMode: false,
    colorSchemePref: (window.__KRUI_INIT__ && window.__KRUI_INIT__.theme) || "system",
    deviceFrame: (window.__KRUI_INIT__ && window.__KRUI_INIT__.deviceFrame) || "none",
    imeFocused: false,
    isPassword: false,
    isComposing: false,
    overlays: { relayout: false, fps: false, hit: false },
    lastSentSize: { w: 0, h: 0 },
    resizeTimer: null,
    lastMoveAt: 0,
    lastError: null,
    imageData: null,
    userInteractedCanvas: false,
  };

  themeSelect.value = state.colorSchemePref;
  frameSelect.value = state.deviceFrame || "none";

  function post(type, extra) {
    vscode.postMessage(Object.assign({ type }, extra || {}));
  }

  function decodeBase64Bytes(b64) {
    const bin = atob(b64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) {
      bytes[i] = bin.charCodeAt(i);
    }
    return bytes;
  }

  function paintKrfrFrame(b64Data) {
    if (!ctx) return;
    const bytes = decodeBase64Bytes(b64Data);
    if (bytes.length < 12) return;
    if (bytes[0] !== 0x4b || bytes[1] !== 0x52 || bytes[2] !== 0x46 || bytes[3] !== 0x52) {
      return;
    }
    const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
    const width = view.getUint16(8, true);
    const height = view.getUint16(10, true);
    if (!width || !height) return;

    if (canvas.width !== width || canvas.height !== height || !state.imageData) {
      canvas.width = width;
      canvas.height = height;
      state.imageData = ctx.createImageData(width, height);
    }
    const out = state.imageData.data;
    const outLen = out.length;
    let dst = 0;
    for (let off = 12; off + 4 < bytes.length && dst < outLen; off += 5) {
      const count = bytes[off] + 1;
      const r = bytes[off + 1];
      const g = bytes[off + 2];
      const b = bytes[off + 3];
      const a = bytes[off + 4];
      const end = Math.min(outLen, dst + count * 4);
      while (dst < end) {
        out[dst] = r;
        out[dst + 1] = g;
        out[dst + 2] = b;
        out[dst + 3] = a;
        dst += 4;
      }
    }
    ctx.putImageData(state.imageData, 0, 0);
    loadingOverlay.classList.remove("visible");
  }

  function canvasCoordsFromEvent(e) {
    const rect = canvas.getBoundingClientRect();
    if (!rect.width || !rect.height || !canvas.width || !canvas.height) {
      return { x: 0, y: 0 };
    }
    return {
      x: ((e.clientX - rect.left) * canvas.width) / rect.width,
      y: ((e.clientY - rect.top) * canvas.height) / rect.height,
    };
  }

  function positionAndSyncIme(msg) {
    state.imeFocused = Boolean(msg.imeFocused) && !msg.isPassword;
    state.isPassword = Boolean(msg.isPassword);

    if (state.isPassword) {
      imeBadge.textContent = "Password (Direct KeyDown)";
    } else if (state.imeFocused) {
      imeBadge.textContent = "IME Active (CJK / Text)";
    } else {
      imeBadge.textContent = "IME Idle";
    }

    if (state.imeFocused) {
      const rect = canvas.getBoundingClientRect();
      const sx = canvas.width > 0 ? rect.width / canvas.width : 1;
      const sy = canvas.height > 0 ? rect.height / canvas.height : 1;
      const caret = Array.isArray(msg.caretRect) ? msg.caretRect : [0, 0, 2, 18];
      imeBridge.style.left = `${Math.round(rect.left + caret[0] * sx)}px`;
      imeBridge.style.top = `${Math.round(rect.top + caret[1] * sy)}px`;
      imeBridge.style.height = `${Math.max(12, Math.round((caret[3] || 18) * sy))}px`;
      if (state.userInteractedCanvas && document.activeElement !== imeBridge) {
        imeBridge.focus({ preventScroll: true });
      }
    } else {
      if (document.activeElement === imeBridge) {
        imeBridge.blur();
        state.isComposing = false;
        imeBridge.value = "";
      }
      imeBridge.style.left = "-9999px";
      imeBridge.style.top = "-9999px";
      if (
        state.userInteractedCanvas &&
        document.activeElement !== stage &&
        !isToolbarControl(document.activeElement)
      ) {
        stage.focus({ preventScroll: true });
      }
    }
  }

  function isToolbarControl(el) {
    return Boolean(el && (el.tagName === "SELECT" || el.tagName === "BUTTON"));
  }

  function updateToolbarFromState(msg) {
    if (
      state.colorSchemePref !== "system" &&
      (msg.theme === "light" || msg.theme === "dark") &&
      themeSelect.value !== msg.theme
    ) {
      themeSelect.value = msg.theme;
    }
    const frameSlug = msg.deviceFrame || "none";
    if (state.deviceFrame !== frameSlug) {
      state.deviceFrame = frameSlug;
      frameSelect.value = frameSlug;
    }
    if (msg.overlays) {
      state.overlays = Object.assign({}, msg.overlays);
      ovRelayout.classList.toggle("active", Boolean(state.overlays.relayout));
      ovFps.classList.toggle("active", Boolean(state.overlays.fps));
      ovHit.classList.toggle("active", Boolean(state.overlays.hit));
    }
  }

  function handleStateSync(msg) {
    const ms = (Number(msg.frameUs || 0) / 1000).toFixed(2);
    toolbarPill.textContent = `${msg.backend || "Renderer"} Live`;
    statSize.textContent = `${msg.width}x${msg.height}`;
    statPerf.textContent = `${ms} ms`;
    statNodes.textContent = `Nodes: ${msg.nodeCount} (relayout: ${msg.relayoutPasses})`;
    statReloads.textContent = `Reloads: ${msg.reloadCount}`;

    const cursorMap = { text: "text", pointer: "pointer", default: "default" };
    canvas.style.cursor = state.inspectMode
      ? "crosshair"
      : cursorMap[msg.cursor] || "default";

    updateToolbarFromState(msg);
    positionAndSyncIme(msg);
    loadingOverlay.classList.remove("visible");
  }

  function showCompileError(msg) {
    state.lastError = {
      file: msg.file || "",
      line: Number(msg.line || 1),
      column: Number(msg.column || 1),
    };
    errorBanner.textContent = `${state.lastError.file}:${state.lastError.line}:${state.lastError.column} — ${msg.message}`;
    errorBanner.classList.add("visible");
  }

  function clearCompileError() {
    state.lastError = null;
    errorBanner.classList.remove("visible");
    errorBanner.textContent = "";
  }

  function utf8ByteLengthUpTo(str, utf16Index) {
    return encoder.encode(str.slice(0, Math.max(0, utf16Index || 0))).length;
  }

  imeBridge.addEventListener("compositionstart", () => {
    state.isComposing = true;
  });

  imeBridge.addEventListener("compositionupdate", (e) => {
    const text = typeof e.data === "string" ? e.data : imeBridge.value || "";
    const startUtf16 = imeBridge.selectionStart != null ? imeBridge.selectionStart : text.length;
    const endUtf16 = imeBridge.selectionEnd != null ? imeBridge.selectionEnd : text.length;
    post("imePreedit", {
      text,
      cursorStart: utf8ByteLengthUpTo(text, Math.min(startUtf16, text.length)),
      cursorEnd: utf8ByteLengthUpTo(text, Math.min(endUtf16, text.length)),
    });
  });

  imeBridge.addEventListener("compositionend", (e) => {
    state.isComposing = false;
    const text = e.data != null ? e.data : imeBridge.value;
    imeBridge.value = "";
    if (text && text.length > 0) {
      post("imeCommit", { text });
    } else {
      post("imePreedit", { text: "", cursorStart: 0, cursorEnd: 0 });
    }
  });

  imeBridge.addEventListener("input", () => {
    if (state.isComposing) return;
    const text = imeBridge.value;
    imeBridge.value = "";
    if (text && text.length > 0) {
      post("imeCommit", { text });
    }
  });

  function handlePaste(e) {
    e.preventDefault();
    if (!state.imeFocused && !state.isPassword) return;
    const text = (e.clipboardData && e.clipboardData.getData("text/plain")) || "";
    if (text.length > 0) {
      post("imeCommit", { text });
    }
  }

  imeBridge.addEventListener("paste", handlePaste);
  stage.addEventListener("paste", handlePaste);

  function handleKeyDown(e) {
    if (e.isComposing || e.keyCode === 229) return;
    if (e.target !== imeBridge && e.target !== stage && e.target !== document.body) {
      return;
    }
    const viaBridge = document.activeElement === imeBridge;
    const hasMod = e.ctrlKey || e.metaKey || e.altKey;
    if (viaBridge && !hasMod && e.key.length === 1) {
      return;
    }
    e.preventDefault();
    post("keyDown", {
      key: e.key,
      shift: e.shiftKey,
      ctrl: e.ctrlKey || e.metaKey,
      alt: e.altKey,
      repeat: e.repeat,
    });
  }

  imeBridge.addEventListener("keydown", handleKeyDown);
  stage.addEventListener("keydown", handleKeyDown);

  canvas.addEventListener("pointerdown", (e) => {
    state.userInteractedCanvas = true;
    const pt = canvasCoordsFromEvent(e);
    if (state.inspectMode || e.altKey) {
      e.preventDefault();
      post("inspectClick", pt);
      if (state.inspectMode) {
        state.inspectMode = false;
        btnInspect.classList.remove("active");
        stage.classList.remove("inspecting");
      }
      return;
    }
    try {
      canvas.setPointerCapture(e.pointerId);
    } catch (_) {}
    if (!state.imeFocused || state.isPassword) {
      stage.focus({ preventScroll: true });
    }
    post("pointerDown", {
      x: pt.x,
      y: pt.y,
      button: e.button,
      shift: e.shiftKey,
      ctrl: e.ctrlKey || e.metaKey,
      alt: e.altKey,
      timeMs: Math.round(e.timeStamp || Date.now()),
    });
  });

  canvas.addEventListener("pointermove", (e) => {
    const now = performance.now();
    if (now - state.lastMoveAt < 8 && (e.buttons || 0) === 0) return;
    state.lastMoveAt = now;
    post("pointerMove", canvasCoordsFromEvent(e));
  });

  canvas.addEventListener("pointerup", (e) => {
    try {
      if (canvas.hasPointerCapture(e.pointerId)) {
        canvas.releasePointerCapture(e.pointerId);
      }
    } catch (_) {}
    post("pointerUp", canvasCoordsFromEvent(e));
  });

  canvas.addEventListener("pointerleave", () => {
    post("pointerLeave");
  });

  canvas.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      const pt = canvasCoordsFromEvent(e);
      post("wheel", { x: pt.x, y: pt.y, deltaY: e.deltaY });
    },
    { passive: false }
  );

  themeSelect.addEventListener("change", () => {
    state.colorSchemePref = themeSelect.value;
    post("setColorScheme", { slug: themeSelect.value });
  });

  frameSelect.addEventListener("change", () => {
    const slug = frameSelect.value;
    state.deviceFrame = slug;
    post("setDeviceFrame", { slug });
    if (slug === "none") {
      scheduleResize();
    }
  });

  function bindOverlayButton(btn, slug) {
    btn.addEventListener("click", () => {
      const enabled = !state.overlays[slug];
      state.overlays[slug] = enabled;
      btn.classList.toggle("active", enabled);
      post("setOverlay", { slug, enabled });
    });
  }

  bindOverlayButton(ovRelayout, "relayout");
  bindOverlayButton(ovFps, "fps");
  bindOverlayButton(ovHit, "hit");

  btnInspect.addEventListener("click", () => {
    state.inspectMode = !state.inspectMode;
    btnInspect.classList.toggle("active", state.inspectMode);
    stage.classList.toggle("inspecting", state.inspectMode);
  });

  btnRestart.addEventListener("click", () => {
    post("restart");
  });

  errorBanner.addEventListener("click", () => {
    if (state.lastError && state.lastError.file) {
      post("jumpToSource", state.lastError);
    }
  });

  function scheduleResize() {
    if (state.deviceFrame !== "none") return;
    clearTimeout(state.resizeTimer);
    state.resizeTimer = setTimeout(() => {
      if (state.deviceFrame !== "none") return;
      const w = Math.max(240, Math.min(1920, Math.floor(stage.clientWidth - 24)));
      const h = Math.max(200, Math.min(1200, Math.floor(stage.clientHeight - 24)));
      if (w === state.lastSentSize.w && h === state.lastSentSize.h) return;
      state.lastSentSize = { w, h };
      post("resize", { width: w, height: h });
    }, 120);
  }

  const observer = new ResizeObserver(scheduleResize);
  observer.observe(stage);

  window.addEventListener("message", (event) => {
    const msg = event.data;
    if (!msg || typeof msg !== "object") return;
    if (msg.type === "krfr") {
      paintKrfrFrame(msg.data);
      return;
    }
    switch (msg.kind) {
      case "building":
        toolbarPill.textContent = `Building ${msg.target || ""}...`;
        loadingText.textContent = msg.message || "Compiling KRui preview...";
        loadingOverlay.classList.add("visible");
        break;
      case "stateSync":
        handleStateSync(msg);
        break;
      case "compileOk":
        clearCompileError();
        break;
      case "compileError":
        showCompileError(msg);
        loadingOverlay.classList.remove("visible");
        break;
    }
  });

  post("webviewReady");
  scheduleResize();
})();

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..");
const fixtureDir = path.join(repoRoot, "tests", "fixtures", "real_css", "tailwind_v4");
const appKruiPath = path.join(repoRoot, "examples", "09_tailwind_v4", "views", "app.krui");

function buildTailwindAppHtml(cssText) {
  let raw = fs.readFileSync(appKruiPath, "utf8");
  raw = raw.replace(/<link[^>]*>/g, "");
  raw = raw.replace(/\{search_query\}/g, "Tailwind v4");
  raw = raw.replace(/bind:value="Tailwind v4"/g, 'value="Tailwind v4"');
  raw = raw.replace(/open="\{dialog_open\}"/g, "");
  raw = raw.replace(/onclick="\{[^"]*\}"/g, "");
  return `<!DOCTYPE html>
<html lang="ko">
<head>
<meta charset="utf-8" />
<style>${cssText}</style>
</head>
<body style="margin:0;padding:0;width:1120px;height:760px;overflow:hidden;">
${raw}
</body>
</html>`;
}

function buildProbesHtml() {
  return `<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8" />
<style>
  body { margin: 0; padding: 0; font-family: sans-serif; font-size: 16px; background: #ffffff; }
  :root { --unused-dark-ref: var(--only-in-dark-mode); }
  @property --c { syntax: "*"; inherits: false; initial-value: #6366f1; }
  @property --zero-len { syntax: "<length>"; inherits: false; initial-value: 0px; }

  .flex-box {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 100px;
    height: 60px;
  }
  #probe-flex-inline {
    display: inline-block;
    height: 20px;
    padding: 0 8px;
    background-color: #3b82f6;
  }
  #probe-undef-var {
    width: 80px;
    height: 24px;
    background-color: #16a34a;
    color: var(--definitely-undefined-var);
  }
  .ring-probe {
    width: 80px;
    height: 28px;
    --bg: var(--c);
    background-color: var(--bg);
    --s: 0 0 0 2px var(--c);
    box-shadow: var(--s);
  }
  .red-probe {
    --c: #ef4444;
  }
  #probe-calc-zero {
    width: calc(96px + var(--zero-len));
    height: 24px;
    background-color: #0ea5e9;
  }
  #probe-fallback-host {
    width: 180px;
    height: 24px;
  }
  #probe-fallback {
    display: inline-block;
    height: 24px;
    width: 100%;
    width: stretch;
    background-color: #8b5cf6;
  }
  #probe-border-nostyle {
    width: 100px;
    height: 32px;
    border-width: 4px;
    border-color: #ef4444;
    background-color: #1e293b;
  }
  #probe-placeholder {
    width: 180px;
    height: 32px;
    color: #0f172a;
    background-color: #f1f5f9;
    border: 1px solid #cbd5e1;
  }
  #probe-placeholder::placeholder {
    color: #f43f5e;
    opacity: 1;
  }
  #probe-cq-host {
    container-type: inline-size;
    width: 240px;
    height: 28px;
  }
  #probe-container {
    width: 60px;
    height: 24px;
    background-color: #64748b;
  }
  @container (max-width: 300px) {
    #probe-container {
      width: 180px;
      background-color: #10b981;
    }
  }
  #probe-filter {
    width: 80px;
    height: 24px;
    background-color: #ef4444;
    filter: invert(1);
  }
</style>
</head>
<body>
  <div class="flex-box"><span id="probe-flex-inline">x</span></div>
  <div id="probe-undef-var"></div>
  <div id="probe-nested-var" class="ring-probe red-probe"></div>
  <div id="probe-calc-zero"></div>
  <div id="probe-fallback-host"><div id="probe-fallback"></div></div>
  <div id="probe-border-nostyle"></div>
  <input id="probe-placeholder" type="text" placeholder="Hint text" />
  <div id="probe-cq-host"><div id="probe-container"></div></div>
  <div id="probe-filter"></div>
</body>
</html>`;
}

async function launchHeadlessBrowser() {
  for (const channel of ["chrome", "msedge"]) {
    try {
      return await chromium.launch({ headless: true, channel });
    } catch {
      // try next channel
    }
  }
  return await chromium.launch({ headless: true });
}

const APP_INSPECT_IDS = [
  "tw-root",
  "tw-banner",
  "open-popover-btn",
  "open-dialog-btn",
  "tw-search",
  "tw-search-echo",
  "peer-toggle",
  "peer-track",
  "peer-knob",
  "group-card",
  "group-title",
  "group-badge",
];

async function inspectPageNodes(page, ids) {
  return await page.evaluate((nodeIds) => {
    const canvas = document.createElement("canvas");
    canvas.width = 1;
    canvas.height = 1;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });

    function toRgbaBytes(cssColor) {
      if (!cssColor || cssColor === "transparent" || cssColor === "rgba(0, 0, 0, 0)") {
        return [0, 0, 0, 0];
      }
      ctx.clearRect(0, 0, 1, 1);
      ctx.fillStyle = "rgba(0,0,0,0)";
      ctx.fillStyle = cssColor;
      ctx.fillRect(0, 0, 1, 1);
      const d = ctx.getImageData(0, 0, 1, 1).data;
      return [d[0], d[1], d[2], d[3]];
    }

    function parsePx(v) {
      const n = parseFloat(v || "0");
      return Number.isFinite(n) ? Math.round(n * 100) / 100 : 0;
    }

    function extractNonTransparentShadowColors(boxShadow) {
      if (!boxShadow || boxShadow === "none") return [];
      const parts = [];
      let depth = 0;
      let start = 0;
      for (let i = 0; i < boxShadow.length; i++) {
        const ch = boxShadow[i];
        if (ch === "(") depth++;
        else if (ch === ")") depth--;
        else if (ch === "," && depth === 0) {
          parts.push(boxShadow.slice(start, i).trim());
          start = i + 1;
        }
      }
      parts.push(boxShadow.slice(start).trim());
      const out = [];
      for (const part of parts) {
        const colorMatch = part.match(/(oklch\([^)]+\)|oklab\([^)]+\)|rgba?\([^)]+\)|color\([^)]+\)|#[0-9a-fA-F]+)/);
        if (!colorMatch) continue;
        const rgba = toRgbaBytes(colorMatch[1]);
        if (rgba[3] === 0) continue;
        const rest = part.replace(colorMatch[1], "").replace(/\binset\b/g, "").trim();
        const nums = rest.split(/\s+/).map((s) => parseFloat(s)).filter((n) => Number.isFinite(n));
        out.push({
          offset_x: nums[0] ?? 0,
          offset_y: nums[1] ?? 0,
          blur: nums[2] ?? 0,
          spread: nums[3] ?? 0,
          color: rgba,
        });
      }
      return out;
    }

    const result = {};
    for (const id of nodeIds) {
      const el = document.getElementById(id);
      if (!el) continue;
      const cs = getComputedStyle(el);
      const r = el.getBoundingClientRect();
      const ph = getComputedStyle(el, "::placeholder");
      result[id] = {
        display: cs.display,
        rect: [
          Math.round(r.x * 100) / 100,
          Math.round(r.y * 100) / 100,
          Math.round(r.width * 100) / 100,
          Math.round(r.height * 100) / 100,
        ],
        background_color: toRgbaBytes(cs.backgroundColor),
        color: toRgbaBytes(cs.color),
        border_top_width: parsePx(cs.borderTopWidth),
        border_top_color: toRgbaBytes(cs.borderTopColor),
        border_radius: parsePx(cs.borderTopLeftRadius),
        font_size: parsePx(cs.fontSize),
        font_weight: parseInt(cs.fontWeight, 10) || 400,
        opacity: Math.round(parseFloat(cs.opacity || "1") * 1000) / 1000,
        has_gradient: cs.backgroundImage.includes("gradient"),
        shadows: extractNonTransparentShadowColors(cs.boxShadow),
        placeholder_color: toRgbaBytes(ph.color),
        filter: cs.filter || "none",
      };
    }
    return result;
  }, ids);
}

const PROBE_IDS = [
  "probe-flex-inline",
  "probe-undef-var",
  "probe-nested-var",
  "probe-calc-zero",
  "probe-fallback",
  "probe-border-nostyle",
  "probe-placeholder",
  "probe-container",
  "probe-filter",
];

const browser = await launchHeadlessBrowser();
try {
  const context = await browser.newContext({
    viewport: { width: 1120, height: 760 },
    deviceScaleFactor: 1,
  });
  const page = await context.newPage();

  const normalCss = fs.readFileSync(path.join(fixtureDir, "tailwind_v4.css"), "utf8");
  await page.setContent(buildTailwindAppHtml(normalCss), { waitUntil: "load" });
  const appNormalNodes = await inspectPageNodes(page, APP_INSPECT_IDS);

  const optCss = fs.readFileSync(path.join(fixtureDir, "tailwind_v4.optimize.css"), "utf8");
  await page.setContent(buildTailwindAppHtml(optCss), { waitUntil: "load" });
  const appOptimizeNodes = await inspectPageNodes(page, APP_INSPECT_IDS);

  await page.setContent(buildProbesHtml(), { waitUntil: "load" });
  const probeNodes = await inspectPageNodes(page, PROBE_IDS);

  const oracle = {
    viewport: [1120, 760],
    browser_version: browser.version(),
    tailwind_normal: appNormalNodes,
    tailwind_optimize: appOptimizeNodes,
    probes: probeNodes,
  };

  const outPath = path.join(fixtureDir, "oracle.json");
  fs.writeFileSync(outPath, JSON.stringify(oracle, null, 2) + "\n", "utf8");
  console.log(`Wrote oracle to ${outPath} (Chromium ${browser.version()})`);
} finally {
  await browser.close();
}

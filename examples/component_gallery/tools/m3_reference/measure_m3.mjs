#!/usr/bin/env node
/**
 * Headless Chromium (Playwright) reference measurement tool for `https://m3.material.io`
 * Viewport: 1280x840, Light mode (`colorScheme: "light"`).
 *
 * Strictly respects anti-bot / CAPTCHA guardrails:
 * If any HTTP 403/429 or challenge gate is encountered, immediately records
 * `status: "unmeasured_blocked"` without clicking, waiting out, or bypassing.
 *
 * Outputs:
 *   - `examples/component_gallery/reference/m3_spec/<slug>.json` (committed)
 *   - `examples/component_gallery/reference/m3_spec/_site_inventory.json` (committed)
 *   - `examples/component_gallery/reference/screens/<slug>.png` (gitignored)
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "..", "..", "..", "..");
const SPEC_DIR = path.join(
  REPO_ROOT,
  "examples",
  "component_gallery",
  "reference",
  "m3_spec"
);
const SCREENS_DIR = path.join(
  REPO_ROOT,
  "examples",
  "component_gallery",
  "reference",
  "screens"
);

const BASE_URL = "https://m3.material.io";
const VIEWPORT = { width: 1280, height: 840 };

const TOP_SECTIONS = [
  { slug: "section_home", sectionId: 0, title: "Home", path: "/" },
  { slug: "section_get_started", sectionId: 1, title: "Get started", path: "/get-started" },
  { slug: "section_develop", sectionId: 4, title: "Develop", path: "/develop" },
  { slug: "section_foundations", sectionId: 5, title: "Foundations", path: "/foundations" },
  { slug: "section_styles", sectionId: 2, title: "Styles", path: "/styles" },
  { slug: "section_components", sectionId: 3, title: "Components", path: "/components" },
  { slug: "section_blog", sectionId: 6, title: "Blog", path: "/blog" },
];

/**
 * `examples/component_gallery/controller/sections.rs` `DOC_PAGES` (0..=40)
 * with candidate `m3.material.io` paths (verified/refined dynamically against live drawer links).
 */
const DOC_PAGES = [
  { pageId: 0, title: "Components overview", candidates: ["/components"] },
  { pageId: 1, title: "App bars", candidates: ["/components/app-bars/overview", "/components/top-app-bar/overview"] },
  { pageId: 2, title: "Badges", candidates: ["/components/badges/overview"] },
  { pageId: 3, title: "Buttons", candidates: ["/components/all-buttons", "/components/buttons/overview"] },
  { pageId: 4, title: "Cards", candidates: ["/components/cards/overview"] },
  { pageId: 5, title: "Carousel", candidates: ["/components/carousel/overview"] },
  { pageId: 6, title: "Checkbox", candidates: ["/components/checkbox/overview"] },
  { pageId: 7, title: "Chips", candidates: ["/components/chips/overview"] },
  { pageId: 8, title: "Date & time pickers", candidates: ["/components/date-pickers/overview"] },
  { pageId: 9, title: "Dialogs", candidates: ["/components/dialogs/overview"] },
  { pageId: 10, title: "Divider", candidates: ["/components/divider/overview"] },
  { pageId: 11, title: "Lists", candidates: ["/components/lists/overview"] },
  { pageId: 12, title: "Loading & progress", candidates: ["/components/progress-indicators/overview", "/components/loading-indicator/overview"] },
  { pageId: 13, title: "Menus", candidates: ["/components/menus/overview"] },
  { pageId: 14, title: "Navigation", candidates: ["/components/navigation-bar/overview"] },
  { pageId: 15, title: "Radio button", candidates: ["/components/radio-button/overview"] },
  { pageId: 16, title: "Search", candidates: ["/components/search/overview"] },
  { pageId: 17, title: "Sheets", candidates: ["/components/bottom-sheets/overview"] },
  { pageId: 18, title: "Sliders", candidates: ["/components/sliders/overview"] },
  { pageId: 19, title: "Snackbar", candidates: ["/components/snackbar/overview"] },
  { pageId: 20, title: "Switch", candidates: ["/components/switch/overview"] },
  { pageId: 21, title: "Tabs", candidates: ["/components/tabs/overview"] },
  { pageId: 22, title: "Text fields", candidates: ["/components/text-fields/overview"] },
  { pageId: 23, title: "Time pickers", candidates: ["/components/time-pickers/overview"] },
  { pageId: 24, title: "Tooltips", candidates: ["/components/tooltips/overview"] },
  { pageId: 25, title: "Common buttons", candidates: ["/components/buttons/overview", "/components/common-buttons/overview"] },
  { pageId: 26, title: "Extended FAB", candidates: ["/components/extended-fab/overview"] },
  { pageId: 27, title: "FAB", candidates: ["/components/floating-action-button/overview", "/components/fab/overview"] },
  { pageId: 28, title: "FAB menu", candidates: ["/components/fab-menu/overview"] },
  { pageId: 29, title: "Icon buttons", candidates: ["/components/icon-buttons/overview"] },
  { pageId: 30, title: "Segmented buttons", candidates: ["/components/segmented-buttons/overview"] },
  { pageId: 31, title: "Split buttons", candidates: ["/components/split-button/overview", "/components/split-buttons/overview"] },
  { pageId: 32, title: "Date pickers", candidates: ["/components/date-pickers/overview"] },
  { pageId: 33, title: "Time pickers (sub)", candidates: ["/components/time-pickers/overview"] },
  { pageId: 34, title: "Loading indicator", candidates: ["/components/loading-indicator/overview"] },
  { pageId: 35, title: "Progress indicators", candidates: ["/components/progress-indicators/overview"] },
  { pageId: 36, title: "Navigation bar", candidates: ["/components/navigation-bar/overview"] },
  { pageId: 37, title: "Navigation drawer", candidates: ["/components/navigation-drawer/overview"] },
  { pageId: 38, title: "Navigation rail", candidates: ["/components/navigation-rail/overview"] },
  { pageId: 39, title: "Bottom sheets", candidates: ["/components/bottom-sheets/overview"] },
  { pageId: 40, title: "Side sheets", candidates: ["/components/side-sheets/overview"] },
];

/**
 * Browser-side extractor traversing both Light DOM and open Shadow DOMs.
 */
function extractPageMetricsInBrowser() {
  const allElements = [];
  function collectElements(root) {
    if (!root) return;
    const children = root.children ? Array.from(root.children) : [];
    for (const el of children) {
      allElements.push(el);
      if (el.shadowRoot) {
        collectElements(el.shadowRoot);
      }
      collectElements(el);
    }
  }
  collectElements(document);

  // Anti-bot / challenge gate check (never interact with or bypass!)
  const bodyTextLower = (document.body?.innerText || "").slice(0, 2000).toLowerCase();
  const hasChallenge =
    Boolean(document.querySelector("iframe[src*='recaptcha'], iframe[src*='turnstile'], iframe[src*='hcaptcha'], .cf-turnstile, .g-recaptcha")) ||
    bodyTextLower.includes("verify you are human") ||
    bodyTextLower.includes("checking your browser");

  if (hasChallenge) {
    return { blockedByChallenge: true };
  }

  function round2(n) {
    return Math.round(n * 100) / 100;
  }

  function snapElement(el, extra = {}) {
    if (!el) return null;
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    return {
      tag: el.tagName.toLowerCase(),
      id: el.id || null,
      className: typeof el.className === "string" ? el.className.trim().slice(0, 160) : null,
      text: (el.innerText || el.textContent || "").trim().replace(/\s+/g, " ").slice(0, 120),
      ariaLabel: el.getAttribute("aria-label") || null,
      href: el.getAttribute("href") || null,
      rect: {
        x: round2(r.x),
        y: round2(r.y),
        width: round2(r.width),
        height: round2(r.height),
        centerX: round2(r.x + r.width / 2),
        centerY: round2(r.y + r.height / 2),
      },
      style: {
        display: cs.display,
        position: cs.position,
        width: cs.width,
        height: cs.height,
        padding: `${cs.paddingTop} ${cs.paddingRight} ${cs.paddingBottom} ${cs.paddingLeft}`,
        margin: `${cs.marginTop} ${cs.marginRight} ${cs.marginBottom} ${cs.marginLeft}`,
        gap: cs.gap,
        rowGap: cs.rowGap,
        columnGap: cs.columnGap,
        borderRadius: cs.borderRadius,
        backgroundColor: cs.backgroundColor,
        color: cs.color,
        borderTop: `${cs.borderTopWidth} ${cs.borderTopStyle} ${cs.borderTopColor}`,
        borderBottom: `${cs.borderBottomWidth} ${cs.borderBottomStyle} ${cs.borderBottomColor}`,
        boxShadow: cs.boxShadow,
        fontFamily: cs.fontFamily,
        fontSize: cs.fontSize,
        fontWeight: cs.fontWeight,
        lineHeight: cs.lineHeight,
        letterSpacing: cs.letterSpacing,
        fontVariationSettings: cs.fontVariationSettings,
        textAlign: cs.textAlign,
      },
      ...extra,
    };
  }

  function isVisible(el) {
    const r = el.getBoundingClientRect();
    if (r.width <= 0.5 || r.height <= 0.5) return false;
    const cs = getComputedStyle(el);
    return cs.display !== "none" && cs.visibility !== "hidden" && parseFloat(cs.opacity || "1") > 0.01;
  }

  // 1. Collect all internal site links for full inventory & URL resolution
  const links = [];
  const seenHref = new Set();
  for (const el of allElements) {
    if (el.tagName === "A") {
      const href = el.getAttribute("href") || "";
      if (href.startsWith("/") && !seenHref.has(href)) {
        seenHref.add(href);
        links.push({
          href,
          text: (el.innerText || el.textContent || "").trim().replace(/\s+/g, " ").slice(0, 100),
          ariaLabel: el.getAttribute("aria-label") || null,
        });
      }
    }
  }

  // 2. Collect Material Symbols / Google Symbols / SVG icons
  const icons = [];
  for (const el of allElements) {
    if (!isVisible(el)) continue;
    const cs = getComputedStyle(el);
    const ff = (cs.fontFamily || "").toLowerCase();
    const cls = typeof el.className === "string" ? el.className.toLowerCase() : "";
    const isSymbolFont =
      ff.includes("symbol") ||
      ff.includes("material icons") ||
      ff.includes("google-symbols") ||
      cls.includes("google-symbols") ||
      cls.includes("material-symbols") ||
      el.tagName.toLowerCase().includes("icon");
    if (isSymbolFont && el.children.length === 0) {
      const ligature = (el.textContent || "").trim();
      if (ligature && ligature.length <= 40 && /^[a-z0-9_]+$/i.test(ligature)) {
        const r = el.getBoundingClientRect();
        icons.push({
          kind: "ligature",
          name: ligature,
          fontFamily: cs.fontFamily,
          fontSize: cs.fontSize,
          fontWeight: cs.fontWeight,
          fontVariationSettings: cs.fontVariationSettings,
          color: cs.color,
          rect: {
            x: round2(r.x),
            y: round2(r.y),
            width: round2(r.width),
            height: round2(r.height),
          },
        });
      }
    }
  }

  // 3. Identify Rail (leftmost vertical nav <= 120px wide, height >= 400px)
  const railCandidates = allElements.filter((el) => {
    if (!isVisible(el)) return false;
    const r = el.getBoundingClientRect();
    return r.x <= 4 && r.width >= 60 && r.width <= 120 && r.height >= 400;
  });
  railCandidates.sort((a, b) => {
    const ra = a.getBoundingClientRect();
    const rb = b.getBoundingClientRect();
    return rb.height - ra.height || ra.width - rb.width;
  });
  const railEl = railCandidates[0] || null;

  // Rail items: links or buttons inside x < 110 with labels like Home, Get started, Develop, Foundations, Styles, Components, Blog
  const railLabelsWanted = new Set(["home", "get started", "develop", "foundations", "styles", "components", "blog"]);
  const railItems = [];
  const railPills = [];
  const railLabels = [];
  let searchButton = null;

  for (const el of allElements) {
    if (!isVisible(el)) continue;
    const r = el.getBoundingClientRect();
    if (r.x < -2 || r.x + r.width > 116) continue;
    const txt = (el.innerText || el.textContent || "").trim().replace(/\s+/g, " ");
    const low = txt.toLowerCase();
    const aria = (el.getAttribute("aria-label") || "").toLowerCase();

    if (
      !searchButton &&
      (el.tagName === "BUTTON" || el.tagName === "A" || el.getAttribute("role") === "button") &&
      (aria.includes("search") || low === "search" || (el.className || "").toString().includes("section-fab")) &&
      r.y < 160 &&
      r.width >= 36
    ) {
      searchButton = snapElement(el);
    }

    if ((el.tagName === "A" || el.tagName === "BUTTON") && railLabelsWanted.has(low.replace(/^[a-z0-9_]+\s+/i, ""))) {
      railItems.push(snapElement(el));
    }
    if (el.children.length === 0 && railLabelsWanted.has(low)) {
      railLabels.push(snapElement(el));
    }
    // Pill candidate inside rail: width 40..72, height 24..40, rounded
    const cs = getComputedStyle(el);
    const br = parseFloat(cs.borderRadius || "0");
    if (r.width >= 40 && r.width <= 72 && r.height >= 24 && r.height <= 42 && br >= 12) {
      railPills.push(snapElement(el));
    }
  }

  // Also capture any interactive controls near bottom of rail (dark/light mode toggle, etc.)
  const railBottomControls = allElements
    .filter((el) => {
      if (!isVisible(el)) return false;
      const r = el.getBoundingClientRect();
      return (
        r.x >= 0 &&
        r.x + r.width <= 116 &&
        r.y >= 560 &&
        (el.tagName === "BUTTON" || el.tagName === "A" || el.getAttribute("role") === "button" || el.getAttribute("role") === "switch")
      );
    })
    .slice(0, 8)
    .map((el) => snapElement(el));

  // 4. Secondary Drawer (if visible, typically x around 80..110, width 180..300)
  const drawerCandidates = allElements.filter((el) => {
    if (!isVisible(el)) return false;
    const r = el.getBoundingClientRect();
    return r.x >= 70 && r.x <= 120 && r.width >= 160 && r.width <= 320 && r.height >= 400;
  });
  const drawerEl = drawerCandidates[0] || null;
  const drawerItems = allElements
    .filter((el) => {
      if (!isVisible(el)) return false;
      const r = el.getBoundingClientRect();
      return (
        r.x >= 72 &&
        r.x + r.width <= 380 &&
        r.height >= 28 &&
        r.height <= 64 &&
        (el.tagName === "A" || el.tagName === "BUTTON")
      );
    })
    .slice(0, 20)
    .map((el) => snapElement(el));

  // 5. Main container, Hero container, Title (`h1`), Summary
  const mainEl =
    allElements.find((el) => el.tagName === "MAIN" && isVisible(el)) ||
    allElements.find((el) => {
      if (!isVisible(el)) return false;
      const r = el.getBoundingClientRect();
      return r.x >= 70 && r.width >= 600 && r.height >= 500;
    }) ||
    null;

  const h1El = allElements.find((el) => el.tagName === "H1" && isVisible(el)) || null;
  let heroContainerEl = null;
  if (h1El) {
    let hp = h1El.parentElement || (h1El.getRootNode && h1El.getRootNode().host);
    while (hp && hp !== document.body) {
      const cs = getComputedStyle(hp);
      if (parseFloat(cs.borderRadius || "0") >= 16 && cs.backgroundColor !== "rgba(0, 0, 0, 0)") {
        heroContainerEl = hp;
        break;
      }
      hp = hp.parentElement || (hp.getRootNode && hp.getRootNode().host);
    }
  }
  let summaryEl = null;
  if (h1El) {
    const h1r = h1El.getBoundingClientRect();
    summaryEl =
      allElements.find((el) => {
        if (!isVisible(el) || el === h1El || el === heroContainerEl) return false;
        if (!["P", "DIV", "SPAN", "H2"].includes(el.tagName)) return false;
        const r = el.getBoundingClientRect();
        const txt = (el.innerText || "").trim().replace(/\s+/g, " ");
        return (
          txt.length >= 16 &&
          txt.length <= 360 &&
          txt !== (h1El.innerText || "").trim() &&
          Math.abs(r.x - h1r.x) <= 60 &&
          r.y >= h1r.bottom - 8 &&
          r.y <= h1r.bottom + 100
        );
      }) || null;
  }

  // 6. Document Tab Bar & Tabs (Overview / Specs / Guidelines / Accessibility)
  const tabKeywords = new Set(["overview", "specs", "guidelines", "accessibility"]);
  const tabEls = allElements.filter((el) => {
    if (!isVisible(el)) return false;
    if (!["A", "BUTTON"].includes(el.tagName) && el.getAttribute("role") !== "tab") return false;
    const txt = (el.innerText || el.textContent || "").trim().replace(/\s+/g, " ").toLowerCase();
    for (const kw of tabKeywords) {
      if (txt === kw || txt.endsWith(" " + kw)) return true;
    }
    return false;
  });

  let tabBarEl =
    allElements.find(
      (el) => isVisible(el) && typeof el.className === "string" && el.className.includes("tabbed-navigation")
    ) || null;
  if (!tabBarEl && tabEls.length >= 2) {
    let p = tabEls[0].parentElement || (tabEls[0].getRootNode && tabEls[0].getRootNode().host);
    while (p && p !== document.body) {
      const r = p.getBoundingClientRect();
      if (r.width >= 240 && r.height >= 36 && r.height <= 96) {
        tabBarEl = p;
        if (r.width >= 360) break;
      }
      p = p.parentElement || (p.getRootNode && p.getRootNode().host);
    }
  }

  const tabsDetailed = tabEls.slice(0, 6).map((tab) => {
    // Check child icon & child label inside tab
    const subAll = [];
    (function walkSub(n) {
      for (const c of Array.from(n.children || [])) {
        subAll.push(c);
        if (c.shadowRoot) walkSub(c.shadowRoot);
        walkSub(c);
      }
    })(tab);
    const iconChild = subAll.find((c) => {
      const ccs = getComputedStyle(c);
      return (
        (ccs.fontFamily || "").toLowerCase().includes("symbol") ||
        c.tagName.toLowerCase() === "svg" ||
        c.tagName.toLowerCase().includes("icon")
      );
    });
    const labelChild = subAll.find((c) => {
      const t = (c.textContent || "").trim().toLowerCase();
      return c.children.length === 0 && tabKeywords.has(t);
    });
    return snapElement(tab, {
      iconChild: snapElement(iconChild),
      labelChild: snapElement(labelChild),
    });
  });

  // 7. "On this page" Table of Contents (TOC)
  const tocHeadingEl =
    allElements.find((el) => {
      if (!isVisible(el)) return false;
      const t = (el.textContent || "").trim().toLowerCase();
      return el.children.length === 0 && t === "on this page";
    }) || null;

  let tocContainerEl = null;
  let tocLinks = [];
  if (tocHeadingEl) {
    let p = tocHeadingEl.parentElement || (tocHeadingEl.getRootNode && tocHeadingEl.getRootNode().host);
    while (p && p !== document.body) {
      const tag = p.tagName.toLowerCase();
      if (tag === "mio-toc" || tag === "nav") {
        tocContainerEl = p;
        break;
      }
      const r = p.getBoundingClientRect();
      if (r.height >= 60 && r.height <= 420 && r.width >= 100) {
        tocContainerEl = p;
        break;
      }
      p = p.parentElement || (p.getRootNode && p.getRootNode().host);
    }
    if (tocContainerEl) {
      const hr = tocContainerEl.getBoundingClientRect();
      tocLinks = allElements
        .filter((el) => {
          if (!isVisible(el) || el.tagName !== "A") return false;
          const r = el.getBoundingClientRect();
          return r.x >= hr.x - 8 && r.x <= hr.right + 8 && r.y >= hr.y && r.y <= hr.bottom + 40;
        })
        .slice(0, 12)
        .map((el) => snapElement(el));
    }
  }

  // 8. Sample cards, buttons, FABs in main content area (x >= 80)
  const sampleCards = allElements
    .filter((el) => {
      if (!isVisible(el)) return false;
      const r = el.getBoundingClientRect();
      if (r.x < 80 || r.width < 160 || r.width > 640 || r.height < 90 || r.height > 480) return false;
      const cs = getComputedStyle(el);
      const br = parseFloat(cs.borderRadius || "0");
      return br >= 12 && cs.backgroundColor !== "rgba(0, 0, 0, 0)";
    })
    .slice(0, 8)
    .map((el) => snapElement(el));

  const sampleButtons = allElements
    .filter((el) => {
      if (!isVisible(el)) return false;
      const r = el.getBoundingClientRect();
      return (
        r.x >= 80 &&
        r.height >= 32 &&
        r.height <= 64 &&
        r.width >= 40 &&
        r.width <= 320 &&
        (el.tagName === "BUTTON" || el.tagName === "A" || el.getAttribute("role") === "button")
      );
    })
    .slice(0, 10)
    .map((el) => snapElement(el));

  const sampleFabs = allElements
    .filter((el) => {
      if (!isVisible(el)) return false;
      const r = el.getBoundingClientRect();
      const cs = getComputedStyle(el);
      const br = parseFloat(cs.borderRadius || "0");
      return (
        r.x >= 80 &&
        ((Math.abs(r.width - r.height) <= 4 && r.width >= 40 && r.width <= 104 && br >= 12) ||
          (el.className || "").toString().toLowerCase().includes("fab"))
      );
    })
    .slice(0, 8)
    .map((el) => snapElement(el));

  return {
    blockedByChallenge: false,
    title: document.title,
    rootBg: getComputedStyle(document.documentElement).backgroundColor,
    bodyBg: getComputedStyle(document.body).backgroundColor,
    rail: snapElement(railEl),
    searchButton,
    railItems: railItems.slice(0, 12),
    railPills: railPills.slice(0, 12),
    railLabels: railLabels.slice(0, 12),
    railBottomControls,
    drawer: snapElement(drawerEl),
    drawerItems,
    mainContainer: snapElement(mainEl),
    heroContainer: snapElement(heroContainerEl),
    headingH1: snapElement(h1El),
    summary: snapElement(summaryEl),
    tabBar: snapElement(tabBarEl),
    tabs: tabsDetailed,
    toc: {
      heading: snapElement(tocHeadingEl),
      container: snapElement(tocContainerEl),
      links: tocLinks,
    },
    sampleCards,
    sampleButtons,
    sampleFabs,
    icons: icons.slice(0, 80),
    links,
  };
}

function normalizeTitleKey(s) {
  return s
    .toLowerCase()
    .replace(/&amp;/g, "and")
    .replace(/&/g, "and")
    .replace(/[^a-z0-9]+/g, " ")
    .trim();
}

async function measureUrl(page, slug, targetPath, meta = {}) {
  const url = `${BASE_URL}${targetPath}`;
  const outJsonPath = path.join(SPEC_DIR, `${slug}.json`);
  const outPngPath = path.join(SCREENS_DIR, `${slug}.png`);

  if (process.argv.includes("--resume") && fs.existsSync(outJsonPath)) {
    try {
      const existing = JSON.parse(fs.readFileSync(outJsonPath, "utf8"));
      if (existing.status === "measured" && existing.metrics?.headingH1?.text) {
        return existing;
      }
    } catch {
      // re-measure
    }
  }

  let response = null;
  try {
    response = await page.goto(url, { waitUntil: "domcontentloaded", timeout: 25000 });
  } catch (err) {
    const failRecord = {
      slug,
      requestedUrl: url,
      status: "unmeasured_network_error",
      error: String(err.message || err),
      viewport: VIEWPORT,
      measuredAt: new Date().toISOString(),
      ...meta,
    };
    fs.writeFileSync(outJsonPath, JSON.stringify(failRecord, null, 2) + "\n");
    return failRecord;
  }

  const httpStatus = response ? response.status() : 0;
  if (httpStatus === 403 || httpStatus === 429) {
    const blockedRecord = {
      slug,
      requestedUrl: url,
      finalUrl: page.url(),
      httpStatus,
      status: "unmeasured_blocked",
      viewport: VIEWPORT,
      measuredAt: new Date().toISOString(),
      ...meta,
    };
    fs.writeFileSync(outJsonPath, JSON.stringify(blockedRecord, null, 2) + "\n");
    return blockedRecord;
  }

  // Wait for Angular route hydration (`h1` appearance) & Material Symbols webfont load
  await page.waitForSelector("h1", { timeout: 7000 }).catch(() => {});
  await page.waitForTimeout(500);
  try {
    await page.evaluate(() => document.fonts && document.fonts.ready);
  } catch {
    // ignore if navigation settled
  }
  await page.waitForTimeout(200);

  // Expand drawer groups on `/components` so all sub-links are visible for inventory
  if (targetPath === "/components") {
    try {
      await page.evaluate(() => {
        const all = [];
        (function walk(r) {
          for (const el of Array.from(r.children || [])) {
            all.push(el);
            if (el.shadowRoot) walk(el.shadowRoot);
            walk(el);
          }
        })(document);
        for (const btn of all) {
          const r = btn.getBoundingClientRect();
          if (
            r.x >= 72 &&
            r.x <= 360 &&
            btn.tagName === "BUTTON" &&
            btn.getAttribute("aria-expanded") === "false"
          ) {
            btn.click();
          }
        }
      });
      await page.waitForTimeout(400);
    } catch {
      // non-fatal
    }
  }

  const extracted = await page.evaluate(extractPageMetricsInBrowser);
  if (extracted.blockedByChallenge) {
    const challengeRecord = {
      slug,
      requestedUrl: url,
      finalUrl: page.url(),
      httpStatus,
      status: "unmeasured_blocked_challenge",
      viewport: VIEWPORT,
      measuredAt: new Date().toISOString(),
      ...meta,
    };
    fs.writeFileSync(outJsonPath, JSON.stringify(challengeRecord, null, 2) + "\n");
    return challengeRecord;
  }

  const isNotFound =
    httpStatus >= 400 ||
    (extracted.headingH1?.text || "").toLowerCase().includes("page not found") ||
    (extracted.title || "").toLowerCase().includes("404");

  const hasHydratedH1 = Boolean(extracted.headingH1?.text);
  const resolvedStatus = isNotFound
    ? "not_found"
    : hasHydratedH1
      ? "measured"
      : "unmeasured_incomplete_hydration";

  await page.screenshot({ path: outPngPath, fullPage: false });

  const record = {
    slug,
    requestedUrl: url,
    finalUrl: page.url(),
    httpStatus,
    status: resolvedStatus,
    viewport: VIEWPORT,
    colorScheme: "light",
    measuredAt: new Date().toISOString(),
    ...meta,
    metrics: extracted,
  };
  fs.writeFileSync(outJsonPath, JSON.stringify(record, null, 2) + "\n");
  return record;
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

async function main() {
  fs.mkdirSync(SPEC_DIR, { recursive: true });
  fs.mkdirSync(SCREENS_DIR, { recursive: true });

  const browser = await launchHeadlessBrowser();
  const context = await browser.newContext({
    viewport: VIEWPORT,
    colorScheme: "light",
    deviceScaleFactor: 1,
    locale: "en-US",
  });
  const page = await context.newPage();

  const allSiteLinks = new Map();
  const summary = {
    measuredAt: new Date().toISOString(),
    viewport: VIEWPORT,
    sections: [],
    docPages: [],
  };

  console.log(`[measure_m3] Measuring ${TOP_SECTIONS.length} top-level sections...`);
  for (const sec of TOP_SECTIONS) {
    const rec = await measureUrl(page, sec.slug, sec.path, {
      kind: "section",
      sectionId: sec.sectionId,
      expectedTitle: sec.title,
    });
    console.log(`  -> ${sec.slug} (${sec.path}): ${rec.status} [${rec.finalUrl || ""}]`);
    summary.sections.push({
      slug: sec.slug,
      sectionId: sec.sectionId,
      title: sec.title,
      path: sec.path,
      finalUrl: rec.finalUrl,
      status: rec.status,
    });
    for (const l of rec.metrics?.links || []) {
      if (!allSiteLinks.has(l.href)) {
        allSiteLinks.set(l.href, l);
      }
    }
  }

  // Build title -> href lookup from discovered `/components/...` links
  const componentLinkByTitle = new Map();
  for (const [href, info] of allSiteLinks.entries()) {
    if (href.startsWith("/components/")) {
      const key = normalizeTitleKey(info.text || "");
      if (key && !componentLinkByTitle.has(key)) {
        componentLinkByTitle.set(key, href);
      }
    }
  }

  // Cache already-measured paths so shared overview URLs don't hit the network twice
  const measuredByPath = new Map();
  console.log(`[measure_m3] Measuring doc_page 0..=40 (${DOC_PAGES.length} entries)...`);
  for (const dp of DOC_PAGES) {
    const slug = `doc_page_${String(dp.pageId).padStart(2, "0")}`;
    const cleanTitle = dp.title.replace(/\s*\(sub\)\s*$/i, "");
    const discoveredHref = componentLinkByTitle.get(normalizeTitleKey(cleanTitle));
    const candidateList = discoveredHref
      ? [discoveredHref, ...dp.candidates.filter((c) => c !== discoveredHref)]
      : dp.candidates;

    let chosenRec = null;
    for (const cand of candidateList) {
      if (measuredByPath.has(cand)) {
        const prev = measuredByPath.get(cand);
        const cloned = {
          ...prev,
          slug,
          pageId: dp.pageId,
          expectedTitle: dp.title,
          reusedFromPath: cand,
        };
        fs.writeFileSync(path.join(SPEC_DIR, `${slug}.json`), JSON.stringify(cloned, null, 2) + "\n");
        if (fs.existsSync(path.join(SCREENS_DIR, `${prev.slug}.png`))) {
          fs.copyFileSync(
            path.join(SCREENS_DIR, `${prev.slug}.png`),
            path.join(SCREENS_DIR, `${slug}.png`)
          );
        }
        chosenRec = cloned;
        if (cloned.status === "measured") break;
        continue;
      }

      const rec = await measureUrl(page, slug, cand, {
        kind: "doc_page",
        pageId: dp.pageId,
        expectedTitle: dp.title,
      });
      measuredByPath.set(cand, rec);
      for (const l of rec.metrics?.links || []) {
        if (!allSiteLinks.has(l.href)) {
          allSiteLinks.set(l.href, l);
        }
      }
      chosenRec = rec;
      if (rec.status === "measured" || rec.status.startsWith("unmeasured_blocked")) {
        break;
      }
    }

    console.log(
      `  -> ${slug} (${dp.title}): ${chosenRec?.status} [${chosenRec?.finalUrl || chosenRec?.requestedUrl}]`
    );
    summary.docPages.push({
      slug,
      pageId: dp.pageId,
      title: dp.title,
      requestedUrl: chosenRec?.requestedUrl,
      finalUrl: chosenRec?.finalUrl,
      status: chosenRec?.status || "unmeasured",
      h1: chosenRec?.metrics?.headingH1?.text || null,
    });
  }

  const inventory = {
    ...summary,
    discoveredSiteLinksCount: allSiteLinks.size,
    discoveredSiteLinks: Array.from(allSiteLinks.values()).sort((a, b) =>
      a.href.localeCompare(b.href)
    ),
  };
  fs.writeFileSync(
    path.join(SPEC_DIR, "_site_inventory.json"),
    JSON.stringify(inventory, null, 2) + "\n"
  );

  await browser.close();
  const measuredCount = summary.docPages.filter((d) => d.status === "measured").length;
  console.log(
    `[measure_m3] Done! Sections measured: ${
      summary.sections.filter((s) => s.status === "measured").length
    }/${summary.sections.length}, doc_pages measured: ${measuredCount}/${summary.docPages.length}, total site links discovered: ${allSiteLinks.size}`
  );
}

main().catch((err) => {
  console.error("[measure_m3] Fatal error:", err);
  process.exit(1);
});

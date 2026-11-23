#!/usr/bin/env node
/**
 * Deep structural measurement of `https://m3.material.io/components/cards/overview`
 * at 1280x840 Light mode:
 * - Search button (`a[href="/search.html"]`)
 * - Active & inactive rail items, pills, icons, labels, bottom toggle switches
 * - Secondary component drawer (triggered by hovering `Components` rail item, and also checked at 1440px)
 * - Hero header container (`h1` + summary + `Resources` chip)
 * - Document tab bar (`Overview`, `Specs`, `Guidelines`, `Accessibility`) — exact container, active pill, icon & label
 * - `"On this page"` TOC container, overline heading, and `.toc-item` links
 * - Content cards, Takeaways/bullet list, numbered callouts (`1`, `2`, `3`)
 *
 * Writes `examples/component_gallery/reference/m3_spec/_component_anatomy.json`
 * and scrolled reference screenshots in `examples/component_gallery/reference/screens/`.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "..", "..");
const SPEC_DIR = path.join(REPO_ROOT, "examples", "component_gallery", "reference", "m3_spec");
const SCREENS_DIR = path.join(REPO_ROOT, "examples", "component_gallery", "reference", "screens");

async function launchHeadlessBrowser() {
  for (const channel of ["chrome", "msedge"]) {
    try {
      return await chromium.launch({ headless: true, channel });
    } catch {
      // next
    }
  }
  return await chromium.launch({ headless: true });
}

function snapChainInBrowser() {
  function round2(n) {
    return Math.round(n * 100) / 100;
  }

  function snap(el) {
    if (!el) return null;
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    const beforeCs = getComputedStyle(el, "::before");
    return {
      tag: el.tagName.toLowerCase(),
      id: el.id || null,
      className: typeof el.className === "string" ? el.className.trim().slice(0, 160) : null,
      text: (el.innerText || el.textContent || "").trim().replace(/\s+/g, " ").slice(0, 120),
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
        flexDirection: cs.flexDirection,
        alignItems: cs.alignItems,
        justifyContent: cs.justifyContent,
        width: cs.width,
        height: cs.height,
        padding: `${cs.paddingTop} ${cs.paddingRight} ${cs.paddingBottom} ${cs.paddingLeft}`,
        margin: `${cs.marginTop} ${cs.marginRight} ${cs.marginBottom} ${cs.marginLeft}`,
        gap: cs.gap,
        borderRadius: cs.borderRadius,
        backgroundColor: cs.backgroundColor,
        color: cs.color,
        border: `${cs.borderTopWidth} ${cs.borderTopStyle} ${cs.borderTopColor}`,
        boxShadow: cs.boxShadow,
        fontFamily: cs.fontFamily,
        fontSize: cs.fontSize,
        fontWeight: cs.fontWeight,
        lineHeight: cs.lineHeight,
        letterSpacing: cs.letterSpacing,
        fontVariationSettings: cs.fontVariationSettings,
        textAlign: cs.textAlign,
      },
      beforePseudo:
        beforeCs && beforeCs.content !== "none"
          ? {
              content: beforeCs.content,
              width: beforeCs.width,
              height: beforeCs.height,
              borderRadius: beforeCs.borderRadius,
              backgroundColor: beforeCs.backgroundColor,
            }
          : null,
    };
  }

  function ancestors(el, depth = 5) {
    const list = [];
    let cur = el;
    for (let i = 0; i < depth && cur && cur !== document.documentElement; i++) {
      list.push(snap(cur));
      cur = cur.parentElement || (cur.getRootNode && cur.getRootNode().host);
    }
    return list;
  }

  const all = [];
  (function walk(root) {
    for (const el of Array.from(root.children || [])) {
      all.push(el);
      if (el.shadowRoot) walk(el.shadowRoot);
      walk(el);
    }
  })(document);

  const searchLink = all.find((el) => el.tagName === "A" && el.getAttribute("href") === "/search.html");
  const activeRailLink = all.find(
    (el) => el.tagName === "A" && typeof el.className === "string" && el.className.includes("section-link") && el.className.includes("active")
  );
  const activeRailChildren = activeRailLink ? Array.from(activeRailLink.querySelectorAll("*")).map(snap) : [];

  const h1 = all.find((el) => el.tagName === "H1");
  const infoTabIcon = all.find(
    (el) =>
      (el.textContent || "").trim() === "info" &&
      (getComputedStyle(el).fontFamily || "").toLowerCase().includes("symbol")
  );
  const styleTabIcon = all.find(
    (el) =>
      (el.textContent || "").trim() === "style" &&
      (getComputedStyle(el).fontFamily || "").toLowerCase().includes("symbol")
  );

  let tabBarRoot = null;
  if (infoTabIcon) {
    let cur = infoTabIcon;
    while (cur && cur !== document.body) {
      const r = cur.getBoundingClientRect();
      if (r.width >= 500 && r.height >= 40 && r.height <= 120) {
        tabBarRoot = cur;
        break;
      }
      cur = cur.parentElement || (cur.getRootNode && cur.getRootNode().host);
    }
  }
  const tabBarChildren = tabBarRoot
    ? Array.from(tabBarRoot.children).map((c) => ({
        self: snap(c),
        descendants: Array.from(c.querySelectorAll("*")).slice(0, 8).map(snap),
      }))
    : [];

  const tocHeading = all.find((el) => el.children.length === 0 && (el.textContent || "").trim() === "On this page");
  const tocItems = all
    .filter((el) => typeof el.className === "string" && el.className.includes("toc-item"))
    .slice(0, 10)
    .map(snap);

  // Numbered callout badges (e.g. "1", "2", "3" inside small circles)
  const numberedBadges = all
    .filter((el) => {
      if (el.children.length > 1) return false;
      const t = (el.textContent || "").trim();
      if (!["1", "2", "3"].includes(t)) return false;
      const r = el.getBoundingClientRect();
      return r.width >= 18 && r.width <= 44 && Math.abs(r.width - r.height) <= 4;
    })
    .slice(0, 6)
    .map(snap);

  return {
    searchChain: ancestors(searchLink, 4),
    searchChildren: searchLink ? Array.from(searchLink.querySelectorAll("*")).map(snap) : [],
    activeRailLink: snap(activeRailLink),
    activeRailChildren,
    heroChain: ancestors(h1, 5),
    infoTabAncestors: ancestors(infoTabIcon, 6),
    styleTabAncestors: ancestors(styleTabIcon, 6),
    tabBarRoot: snap(tabBarRoot),
    tabBarChildren,
    tocHeadingAncestors: ancestors(tocHeading, 5),
    tocItems,
    numberedBadges,
  };
}

async function main() {
  const browser = await launchHeadlessBrowser();
  const context = await browser.newContext({
    viewport: { width: 1280, height: 840 },
    colorScheme: "light",
    deviceScaleFactor: 1,
    locale: "en-US",
  });
  const page = await context.newPage();

  await page.goto("https://m3.material.io/components/cards/overview", {
    waitUntil: "domcontentloaded",
    timeout: 25000,
  });
  await page.waitForTimeout(1600);
  const anatomy = await page.evaluate(snapChainInBrowser);

  // Scroll to tab bar & TOC so we capture a crisp visual reference screenshot
  if (anatomy.tabBarRoot) {
    await page.evaluate((y) => window.scrollTo(0, Math.max(0, y - 40)), anatomy.tabBarRoot.rect.y);
    await page.waitForTimeout(400);
    await page.screenshot({ path: path.join(SCREENS_DIR, "cards_overview_tabs_and_toc.png") });
  }
  await page.evaluate(() => window.scrollTo(0, 1450));
  await page.waitForTimeout(400);
  await page.screenshot({ path: path.join(SCREENS_DIR, "cards_overview_stage.png") });

  // Hover Components rail item at scroll=0 to inspect the flyout component drawer
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.waitForTimeout(250);
  await page.mouse.move(44, 486);
  await page.waitForTimeout(500);
  await page.screenshot({ path: path.join(SCREENS_DIR, "components_flyout_drawer.png") });

  const drawerAnatomy = await page.evaluate(() => {
    function round2(n) {
      return Math.round(n * 100) / 100;
    }
    function snap(el) {
      if (!el) return null;
      const r = el.getBoundingClientRect();
      const cs = getComputedStyle(el);
      return {
        tag: el.tagName.toLowerCase(),
        className: typeof el.className === "string" ? el.className.trim().slice(0, 140) : null,
        text: (el.innerText || el.textContent || "").trim().replace(/\s+/g, " ").slice(0, 80),
        rect: { x: round2(r.x), y: round2(r.y), width: round2(r.width), height: round2(r.height) },
        style: {
          width: cs.width,
          height: cs.height,
          padding: `${cs.paddingTop} ${cs.paddingRight} ${cs.paddingBottom} ${cs.paddingLeft}`,
          borderRadius: cs.borderRadius,
          backgroundColor: cs.backgroundColor,
          color: cs.color,
          fontFamily: cs.fontFamily,
          fontSize: cs.fontSize,
          fontWeight: cs.fontWeight,
          lineHeight: cs.lineHeight,
        },
      };
    }
    const all = [];
    (function walk(root) {
      for (const el of Array.from(root.children || [])) {
        all.push(el);
        if (el.shadowRoot) walk(el.shadowRoot);
        walk(el);
      }
    })(document);
    const visibleDrawerContainers = all
      .filter((el) => {
        const r = el.getBoundingClientRect();
        const cs = getComputedStyle(el);
        return (
          cs.display !== "none" &&
          cs.visibility !== "hidden" &&
          parseFloat(cs.opacity || "1") > 0.1 &&
          r.x >= 80 &&
          r.x <= 110 &&
          r.width >= 160 &&
          r.width <= 340 &&
          r.height >= 400
        );
      })
      .slice(0, 4)
      .map(snap);

    const visibleDrawerLinks = all
      .filter((el) => {
        const r = el.getBoundingClientRect();
        const cs = getComputedStyle(el);
        return (
          (el.tagName === "A" || el.tagName === "BUTTON") &&
          cs.display !== "none" &&
          cs.visibility !== "hidden" &&
          parseFloat(cs.opacity || "1") > 0.1 &&
          r.x >= 84 &&
          r.x + r.width <= 360 &&
          r.height >= 28 &&
          r.height <= 60
        );
      })
      .slice(0, 16)
      .map(snap);

    return { visibleDrawerContainers, visibleDrawerLinks };
  });

  const full = {
    measuredAt: new Date().toISOString(),
    url: "https://m3.material.io/components/cards/overview",
    viewport: { width: 1280, height: 840 },
    ...anatomy,
    drawerAnatomy,
  };
  fs.writeFileSync(path.join(SPEC_DIR, "_component_anatomy.json"), JSON.stringify(full, null, 2) + "\n");
  await browser.close();
  console.log("[measure_m3_details] Wrote _component_anatomy.json and scrolled reference screenshots.");
}

main().catch((e) => {
  console.error(e);
  process.exit(1);
});

#!/usr/bin/env node
/**
 * Deterministic Material Symbols `.krui` icon component generator.
 *
 * Source package: `@material-symbols/svg-400` (Apache-2.0), style `outlined` (`wght=400`, `opsz=24`,
 * matching `fontVariationSettings` measured on `https://m3.material.io`).
 *
 * Usage:
 *   node tools/gen_m3_icons.mjs                # generates full gallery icon set
 *   node tools/gen_m3_icons.mjs home search    # generates specific symbol(s)
 */

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(__dirname, "..");
const PKG_DIR = path.join(REPO_ROOT, "tools", "node_modules", "@material-symbols", "svg-400");
const OUT_DIR = path.join(REPO_ROOT, "examples", "component_gallery", "views", "icons");

const DEFAULT_SPECS = [
  { symbol: "accessibility_new", filled: false },
  { symbol: "accessibility_new", filled: true },
  { symbol: "add", filled: false },
  { symbol: "add_circle", filled: false },
  { symbol: "add_circle", filled: true },
  { symbol: "ads_click", filled: false },
  { symbol: "apps", filled: false },
  { symbol: "apps", filled: true },
  { symbol: "archive", filled: false },
  { symbol: "arrow_drop_down", filled: false },
  { symbol: "arrow_drop_up", filled: false },
  { symbol: "auto_awesome_mosaic", filled: false },
  { symbol: "book", filled: false },
  { symbol: "book", filled: true },
  { symbol: "bookmark", filled: false },
  { symbol: "bookmark", filled: true },
  { symbol: "calendar_today", filled: false },
  { symbol: "chat", filled: false },
  { symbol: "check", filled: false },
  { symbol: "check_box", filled: false },
  { symbol: "check_circle", filled: false },
  { symbol: "chevron_left", filled: false },
  { symbol: "chevron_right", filled: false },
  { symbol: "close", filled: false },
  { symbol: "code", filled: false },
  { symbol: "code", filled: true },
  { symbol: "credit_card", filled: false },
  { symbol: "dark_mode", filled: false },
  { symbol: "description", filled: false },
  { symbol: "design_services", filled: false },
  { symbol: "design_services", filled: true },
  { symbol: "edit", filled: false },
  { symbol: "explore", filled: false },
  { symbol: "extension", filled: false },
  { symbol: "favorite", filled: false },
  { symbol: "favorite", filled: true },
  { symbol: "folder", filled: false },
  { symbol: "home", filled: false },
  { symbol: "home", filled: true },
  { symbol: "image", filled: false },
  { symbol: "info", filled: false },
  { symbol: "info", filled: true },
  { symbol: "keyboard_arrow_down", filled: false },
  { symbol: "keyboard_arrow_up", filled: false },
  { symbol: "layers", filled: false },
  { symbol: "light_mode", filled: false },
  { symbol: "lightbulb", filled: false },
  { symbol: "lock", filled: false },
  { symbol: "menu", filled: false },
  { symbol: "more_vert", filled: false },
  { symbol: "navigation", filled: false },
  { symbol: "notifications", filled: false },
  { symbol: "open_in_new", filled: false },
  { symbol: "pages", filled: false },
  { symbol: "pages", filled: true },
  { symbol: "palette", filled: false },
  { symbol: "palette", filled: true },
  { symbol: "pause", filled: false },
  { symbol: "person", filled: false },
  { symbol: "play_arrow", filled: false },
  { symbol: "radio_button_checked", filled: false },
  { symbol: "radio_button_unchecked", filled: false },
  { symbol: "schedule", filled: false },
  { symbol: "search", filled: false },
  { symbol: "settings", filled: false },
  { symbol: "star", filled: false },
  { symbol: "star", filled: true },
  { symbol: "style", filled: false },
  { symbol: "style", filled: true },
  { symbol: "text_fields", filled: false },
  { symbol: "toggle_on", filled: false },
  { symbol: "touch_app", filled: false },
  { symbol: "translate", filled: false },
  { symbol: "tune", filled: false },
  { symbol: "view_agenda", filled: false },
  { symbol: "view_column", filled: false },
  { symbol: "widgets", filled: false },
];

function toPascalCase(symbolName) {
  return symbolName
    .split(/[_-]+/)
    .filter(Boolean)
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1).toLowerCase())
    .join("");
}

function componentNameFor(spec) {
  return `Icon${toPascalCase(spec.symbol)}${spec.filled ? "Fill" : ""}`;
}

function svgFileNameFor(spec) {
  return `${spec.symbol}${spec.filled ? "-fill" : ""}.svg`;
}

function parseOfficialSvg(svgRaw, relPath) {
  const vbMatch = svgRaw.match(/\bviewBox="([^"]+)"/);
  if (!vbMatch) {
    throw new Error(`Missing viewBox in ${relPath}`);
  }
  const pathMatches = [...svgRaw.matchAll(/<path\s+d="([^"]+)"\s*\/>/g)];
  if (pathMatches.length === 0) {
    throw new Error(`No <path d="..."/> found in ${relPath}`);
  }
  return {
    viewBox: vbMatch[1],
    paths: pathMatches.map((m) => m[1]),
  };
}

function buildKruiComponent(compName, viewBox, paths, relSource) {
  const pathMarkup = paths.map((d) => `<path d="${d}"/>`).join("");
  return [
    `<!-- Generated by tools/gen_m3_icons.mjs from ${relSource} (Apache-2.0). DO NOT EDIT. -->`,
    `<template name="${compName}" class="">`,
    `    <svg class="m3-icon {class}" viewBox="${viewBox}" fill="currentColor">${pathMarkup}</svg>`,
    `</template>`,
    ``,
  ].join("\n");
}

function buildSourceMd(pkgJson, rows) {
  const lines = [
    `# Material Symbols Icon Components (\`views/icons/\`)`,
    ``,
    `- **Package**: \`${pkgJson.name}@${pkgJson.version}\``,
    `- **License**: \`${pkgJson.license}\` (Google Material Symbols — Apache License 2.0)`,
    `- **Repository**: ${pkgJson.repository?.url || "https://github.com/marella/material-symbols"}`,
    `- **Style / Weight**: \`outlined\`, \`wght=400\` (\`FILL=0\` default, \`FILL=1\` \`*Fill\` active variants)`,
    `- **Generator**: \`node tools/gen_m3_icons.mjs\``,
    ``,
    `## Generated Components (${rows.length})`,
    ``,
    `| Component | Symbol | FILL | Source SVG | Source SHA-256 |`,
    `|---|---|---:|---|---|`,
  ];
  for (const r of rows) {
    lines.push(
      `| \`${r.compName}\` | \`${r.symbol}\` | ${r.filled ? 1 : 0} | \`${r.relSource}\` | \`${r.sha256}\` |`
    );
  }
  lines.push(``);
  return lines.join("\n");
}

function main() {
  const pkgJson = JSON.parse(fs.readFileSync(path.join(PKG_DIR, "package.json"), "utf8"));
  const cliArgs = process.argv.slice(2).filter((a) => !a.startsWith("--"));
  const specs =
    cliArgs.length > 0
      ? cliArgs.map((arg) => {
          const filled = arg.endsWith("-fill");
          const symbol = filled ? arg.slice(0, -5) : arg;
          return { symbol, filled };
        })
      : DEFAULT_SPECS;

  const sorted = [...specs].sort((a, b) =>
    componentNameFor(a).localeCompare(componentNameFor(b))
  );

  fs.mkdirSync(OUT_DIR, { recursive: true });
  const rows = [];
  for (const spec of sorted) {
    const compName = componentNameFor(spec);
    const svgFile = svgFileNameFor(spec);
    const absSvg = path.join(PKG_DIR, "outlined", svgFile);
    if (!fs.existsSync(absSvg)) {
      throw new Error(`Official Material Symbol SVG not found: ${absSvg}`);
    }
    const rawSvg = fs.readFileSync(absSvg, "utf8");
    const sha256 = crypto.createHash("sha256").update(rawSvg, "utf8").digest("hex");
    const relSource = `@material-symbols/svg-400/outlined/${svgFile}`;
    const { viewBox, paths } = parseOfficialSvg(rawSvg, relSource);
    const kruiContent = buildKruiComponent(compName, viewBox, paths, relSource);
    fs.writeFileSync(path.join(OUT_DIR, `${compName}.krui`), kruiContent, "utf8");
    rows.push({ compName, symbol: spec.symbol, filled: spec.filled, relSource, sha256 });
  }

  if (cliArgs.length === 0) {
    const sourceMd = buildSourceMd(pkgJson, rows);
    fs.writeFileSync(path.join(OUT_DIR, "SOURCE.md"), sourceMd, "utf8");
  }
  console.log(`[gen_m3_icons] Generated ${rows.length} icon components in ${OUT_DIR}`);
}

main();

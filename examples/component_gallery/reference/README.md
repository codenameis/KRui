# `m3.material.io` Live Reference Specification (`1280×840`, Light Mode)

## 1. Measurement Provenance & Coverage
- **Target Site**: `https://m3.material.io`
- **Viewport & Color Scheme**: `1280×840`, `prefers-color-scheme: light`
- **Scripts**:
  - `tools/m3_reference/measure_m3.mjs` (captures `section_*.json`, `doc_page_00..40.json`, `_site_inventory.json`, and gitignored `reference/screens/*.png`)
  - `tools/m3_reference/measure_m3_details.mjs` (captures `_component_anatomy.json` deep ancestor/descendant chains and flyout drawer metrics)
  - Distilled runtime gate metrics stored in `examples/component_gallery/reference/m3_core_spec.json`.
- **Coverage**:
  - Top-level sections: **`7 / 7` measured** (`section_home`, `section_get_started`, `section_develop`, `section_foundations`, `section_styles`, `section_components`, `section_blog`)
  - Gallery `doc_page` entries (`0..=40`): **`41 / 41` measured (`0` unmeasured, `0` CAPTCHA/blocks)**
  - Full site link inventory (`_site_inventory.json`): **388 unique internal paths**

---

## 2. Measured Numeric Summary Table (Single Source of Truth for Gallery Tokens)

All values below are taken directly from the `m3.material.io` measurement pass and summarized in `m3_core_spec.json`.

### 2.1 Primary Navigation Rail (`mio-left-nav-rail`)

| Element | Selector on `m3.material.io` | Width × Height | Padding / Margin / Gap | Corner Radius | Typography (`size / line-height / weight / tracking`) | Light Palette (`bg` / `fg`) | Icon (`opsz`, `wght`, `FILL`) |
|---|---|---|---|---|---|---|---|
| Rail container | `mio-left-nav-rail` | `88px × 840px` (`100%`) | top content offset `20px`, `justify-content: space-between` | `0px` | `16px / 24px / 400` | bg `#f2ecee` (`rgb(242, 236, 238)`), fg `#1f1f1f` | — |
| Search FAB | `a.section-fab` | `56px × 56px` (`centerX: 44px`) | `padding: 2px`, `margin-bottom: 24px` | `16px` | — | bg `#f1d3f9` (`rgb(241, 211, 249)`), fg `#4d4256` (`rgb(77, 66, 86)`) | `search` (`24px`, `opsz=24, wght=400, FILL=0`) |
| Rail item | `a.section-link` | `80px × 60px` (`centerX: 44px`) | vertical item stride `72px` (`12px` gap) | `4px` | `16px / 24px / 500` | transparent | — |
| Rail indicator pill | `a.section-link > span.google-symbols` (`::before`) | `56px × 32px` (`centerX: 44px`) | `margin-bottom: 4px` (pill-to-label gap `4px`) | `16px` (`100px` full pill) | — | active bg `#dcdaf5` (`rgb(220, 218, 245)`), active fg `#21182b` (`rgb(33, 24, 43)`); inactive bg `transparent`, fg `#4d4256` | `24px × 24px`, `opsz=24, wght=400`, `FILL=1` active / `FILL=0` inactive |
| Rail label | `a.section-link > div.label` | `76px × 16px` (`centerX: 44px`) | `margin-bottom: 4px` | `0px` | **`12px / 16px / 500`** (`letter-spacing: 0.1px`, `text-align: center`) | active `#21182b`, inactive `#4d4256` | — |
| Rail icons (top → bottom) | `span.google-symbols` | `24px × 24px` | centered in `56×32` pill | — | — | `currentColor` | `home`*, `apps`, `code`, `book`, `palette`, `add_circle`, `pages` |
| Rail bottom circle buttons | `mio-toggle-switch button` | `48px × 48px` (`centerX: 44px`) | `12px` vertical gap | `999px` (circle) | — | `1px solid` outline `#cac4d0`, fg `#4d4256` | `pause` / `play_arrow` (`24px`), `dark_mode` / `light_mode` (`24px`) |

\* Note: `m3.material.io` uses Google's proprietary `material_design` ligature glyph for Home; standard Apache-2.0 Material Symbols provides `home` (`IconHome` / `IconHomeFill`).

### 2.2 Secondary Component Drawer (`aside.mdc-drawer`)

| Element | Selector on `m3.material.io` | Width × Height | Padding / Gap | Corner Radius | Typography | Light Palette (`bg` / `fg`) | Icon |
|---|---|---|---|---|---|---|---|
| Drawer surface | `aside.mdc-drawer` | `240px × 840px` (`100%`) | `padding: 8px` | `0px 16px 16px 0px` | `16px / 24px / 400` | bg `#f2ecee` (`rgb(242, 236, 238)`) | — |
| Drawer item slot | `a.focusable-topic-item` | `223px × 48px` | `padding: 2px` | `24px` | — | — | — |
| Drawer item button | `button.mdc-button` | `height: 44px` (sub-item `44px`) | `padding: 0px 16px` (sub-item `padding-left: 28px`) | `22px` (`999px` pill) | **`14px / 24px / 500`** | active bg `#dcdaf5`, active fg `#21182b`; inactive fg `#4d4256` | `arrow_drop_down` (`20px–24px`) |

### 2.3 Main Document Hero, Tab Bar & Body (`main#main_content`)

| Element | Selector on `m3.material.io` | Width × Height | Padding / Margin / Gap | Corner Radius | Typography (`size / line-height / weight`) | Light Palette (`bg` / `fg`) | Icon (`opsz`, `wght`, `FILL`) |
|---|---|---|---|---|---|---|---|
| Main outer margin | `main#main_content` / `header.split-asset` | `flex: 1` (`1176px` at flyout, `936px` beside docked `240px` drawer) | outer inset `8px`, block gap `8px` | — | `16px / 24px / 400` | page bg `#fefbff` (`rgb(254, 251, 255)`) | — |
| Hero header card | `div.primary-container.description` | `width: 100%`, `min-height: 224px` | **`padding: 56px`** | **`24px`** | — | bg `#f8f1f6` (`rgb(248, 241, 246)`), fg `#1c1b1d` | — |
| Page title (`h1`) | `div.primary-container h1` | auto | `margin-bottom: 16px` | — | **`88px / 96px / 475`** (Display Expressive; wraps cleanly on multi-word titles) | `#1c1b1d` (`rgb(28, 27, 29)`) | — |
| Page summary | `div.primary-container .description` | auto | `0px` | — | **`22px / 30px / 400`** (M3 Title Large) | `#1c1b1d` (`rgb(28, 27, 29)`) | — |
| Document tab bar | `div.tabbed-navigation` | `width: 100%`, **`height: 80px`** (`79px` measured) | `padding: 0px`, `box-shadow: none` | **`40px`** | — | bg `#f8f1f6` (`rgb(248, 241, 246)`) | — |
| Document tab | `div.tabbed-navigation > a.tab` | `flex: 1`, **`height: 80px`** (`79px` measured) | `padding: 0px 24px`, icon-to-label **`gap: 8px`** | **`40px`** | **`16px / 24px / 500`** | active pill bg `#dcdaf5` (`rgb(220, 218, 245)`), active fg `#21182b`; inactive bg `transparent`, fg `#4d4256` | `Overview`: `info`, `Specs`: `style`, `Guidelines`: `design_services`, `Accessibility`: `accessibility_new` (`24px`, `FILL=1` active / `FILL=0` inactive) |

### 2.4 `"On this page"` Table of Contents (`mio-toc`)

| Element | Selector on `m3.material.io` | Width × Height | Padding / Margin | Corner Radius | Typography | Light Palette |
|---|---|---|---|---|---|---|
| Overline heading | `mio-toc div.overline` | `height: 16px` | `margin: 0 16px 8px 16px` | `0px` | **`11px / 16px / 500`**, `letter-spacing: 0.1px` (NO repeated `22px` `h1` title!) | `#4d4256` (`rgb(77, 66, 86)`) |
| TOC link pill | `mio-toc a.toc-item` | **`height: 40px`** | `padding: 8px 16px` | **`20px`** | **`16px / 24px / 500`** | fg `#6442d6` (`rgb(100, 66, 214)`) |

### 2.5 Component Specimens (M3 Spec + Live Site)

| Component | Key Dimensions | Shape / Radius | Typography / Icon |
|---|---|---|---|
| Common Button (`Button`) | `height: 40px`, `padding: 0 24px`, `gap: 8px` | `20px` (`999px` full pill) | `14px / 20px / 500`, icon `18px` |
| Split Button (`SplitButton`) | `height: 40px`, leading `padding: 0 16px`, trailing `width: 40px`, `gap: 2px` | outer `20px`, inner **`4px`** (`20px 4px 4px 20px` / `4px 20px 20px 4px`) | `14px / 20px / 500`, trailing `arrow_drop_down` `20px` |
| Connected Button Group | `height: 40px`, `padding: 0 16px`, `gap: 2px` | outer `20px`, inner `8px`, active `20px` | `14px / 20px / 500`, `white-space: nowrap` |
| FAB (`sm` / `md` / `lg`) | `40×40px` / `56×56px` / **`96×96px`** | `12px` / `16px` / **`28px`** | `add` icon `24px` (`sm`/`md`), **`36px`** (`lg`) |
| Extended FAB | `height: 56px`, `padding: 0 20px`, `gap: 12px` | `16px` | `14px / 20px / 500`, icon `24px` |
| Icon Button | `40×40px` | `999px` | icon `24px` |
| Card (`elevated` / `filled` / `outlined`) | `padding: 16px` | `12px` (`--md-sys-shape-corner-medium`) | title `16px / 24px / 500`, body `14px / 20px / 400` |
| Numbered Callout Circle (`.m3-step-badge`) | `24×24px` | `999px` | `12px / 16px / 700`, inverse surface bg `#322f35` + inverse on-surface fg `#f5eff7` |

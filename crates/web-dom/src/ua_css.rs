//! KRui UA CSS baseline and `@layer` cascade parity rules for native browser DOM rendering.
//!
//! Embeds KRui's canonical `ua_shared.css` and `unstyled.css` directly from `crates/stylo-bridge/ua/`
//! without adding `krui-stylo-bridge` or `stylo` to `krui-web`'s dependency graph.

use std::collections::HashMap;

const UA_SHARED_CSS: &str = include_str!("../../stylo-bridge/ua/ua_shared.css");
const UA_UNSTYLED_CSS: &str = include_str!("../../stylo-bridge/ua/unstyled.css");

const BROWSER_NORMALIZE_CSS: &str = r#"
html {
    font-size: 16px;
    line-height: normal;
    box-sizing: border-box;
    -webkit-text-size-adjust: 100%;
    text-size-adjust: 100%;
    color-scheme: light;
}
html, body, #krui-root {
    margin: 0;
    padding: 0;
    width: 100%;
    height: 100%;
    overflow: hidden;
}
#krui-root {
    position: fixed;
    inset: 0;
    display: flex;
    flex-direction: column;
}
#krui-root > * {
    flex: 1 1 0%;
    min-height: 0;
    min-width: 0;
}
*, *::before, *::after {
    box-sizing: border-box;
}
h1, h2, h3, h4, h5, h6, p, ul, ol, li, dl, dd, figure, blockquote, pre, fieldset, form, hr {
    margin: 0;
}
ul, ol {
    padding-left: 0;
    list-style: none;
}
fieldset {
    padding: 0;
    border: none;
    min-width: 0;
}
button, input, select, textarea {
    font-family: inherit;
    font-size: inherit;
    font-weight: inherit;
    line-height: inherit;
    letter-spacing: inherit;
    color: inherit;
    margin: 0;
    background: transparent;
}
button:focus, input:focus, select:focus, textarea:focus {
    outline: none;
}
button:focus-visible, input:focus-visible, select:focus-visible, textarea:focus-visible {
    outline: none;
    outline-width: 0px;
}
a {
    color: inherit;
    text-decoration: none;
}
img, svg, canvas {
    display: block;
}
dialog {
    position: static;
    margin: 0;
    padding: 0;
    border: none;
    width: auto;
    height: auto;
    max-width: none;
    max-height: none;
    background: transparent;
    color: inherit;
}
dialog:not([open]) {
    display: none;
}
"#;

const OVERRIDES_CSS: &str = r#"
:root[data-krui-scheme="dark"],
:root[data-krui-scheme="dark"] [data-theme] {
    color-scheme: dark !important;
}
:root[data-krui-scheme="light"],
:root[data-krui-scheme="light"] [data-theme] {
    color-scheme: light !important;
}
:root[data-krui-scheme="system"] {
    color-scheme: light dark;
}
:root[data-krui-anim-paused="true"],
:root[data-krui-anim-paused="true"] *,
:root[data-krui-anim-paused="true"] *::before,
:root[data-krui-anim-paused="true"] *::after {
    animation-play-state: paused !important;
    transition-duration: 0s !important;
}
"#;

/// Splits top-level `@property { ... }` rules from the rest of a stylesheet because
/// W3C CSS ignores `@property` rules placed inside `@layer` blocks in several engines.
fn hoist_property_at_rules(raw: &str) -> (String, String) {
    let mut hoisted = String::new();
    let mut rest = String::with_capacity(raw.len());
    let mut cursor = 0usize;
    while let Some(rel) = raw[cursor..].find("@property") {
        let start = cursor + rel;
        rest.push_str(&raw[cursor..start]);
        if let Some(end) = find_at_rule_block_end(raw, start) {
            hoisted.push_str(&raw[start..=end]);
            hoisted.push('\n');
            cursor = end + 1;
        } else {
            rest.push_str(&raw[start..]);
            cursor = raw.len();
        }
    }
    rest.push_str(&raw[cursor..]);
    (hoisted, rest)
}

fn find_at_rule_block_end(src: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut seen_brace = false;
    for (offset, ch) in src[start..].char_indices() {
        match ch {
            '{' => {
                depth += 1;
                seen_brace = true;
            }
            '}' if depth > 0 => {
                depth -= 1;
                if seen_brace && depth == 0 {
                    return Some(start + offset);
                }
            }
            _ => {}
        }
    }
    None
}

/// Builds the full KRui base CSS sheet with `@layer` precedence matching Stylo's cascade order.
pub fn build_ua_stylesheet(include_unstyled: bool) -> String {
    let (props_shared, body_shared) = hoist_property_at_rules(UA_SHARED_CSS);
    let mut out = String::with_capacity(
        UA_SHARED_CSS.len() + UA_UNSTYLED_CSS.len() + BROWSER_NORMALIZE_CSS.len() + 512,
    );
    out.push_str("@layer krui-ua, preset, component, micl;\n");
    out.push_str(&props_shared);
    out.push_str("@layer krui-ua {\n");
    out.push_str(BROWSER_NORMALIZE_CSS);
    out.push_str(&body_shared);
    if include_unstyled {
        out.push('\n');
        out.push_str(UA_UNSTYLED_CSS);
    }
    out.push_str("\n}\n");
    out.push_str(OVERRIDES_CSS);
    out
}

/// Rewrites `krui://inline/<key>` URLs inside compiled CSS to browser `blob:` / data URLs.
pub fn rewrite_inline_asset_urls(css: &str, asset_urls: &HashMap<String, String>) -> String {
    let mut result = css.to_string();
    for (key, url) in asset_urls {
        let token = format!("krui://inline/{key}");
        if result.contains(&token) {
            result = result.replace(&token, url);
        }
    }
    result
}

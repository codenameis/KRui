//! Multilingual font family candidate tables, CSS generic family expansion
//! (`system-ui`, `ui-sans-serif`, `ui-serif`, `ui-monospace`, `sans-serif`, `serif`, `monospace`, `emoji`),
//! symbol-font filtering, and stack resolution.

use cosmic_text::{Family, FontSystem};
use krui_style::FontFamilyStack;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub(crate) struct ResolvedFontStack {
    pub(crate) candidates: Vec<&'static str>,
}

const SYSTEM_UI_SANS: &[&str] = &[
    "Segoe UI Variable Text",
    "Segoe UI Variable",
    "Segoe UI",
    "SF Pro Text",
    "SF Pro Display",
    "Inter",
    "Roboto",
    "Noto Sans",
    "Arial",
    "Apple SD Gothic Neo",
    "Malgun Gothic",
    "맑은 고딕",
    "Pretendard",
    "Noto Sans KR",
    "Noto Sans CJK KR",
    "Yu Gothic UI",
    "Microsoft YaHei UI",
];

const FLUENT_LATIN: &[&str] = &[
    "Segoe UI",
    "Inter",
    "Segoe UI Variable Text",
    "Noto Sans",
    "Arial",
];
const FLUENT_KOREAN: &[&str] = &[
    "Pretendard",
    "Malgun Gothic",
    "맑은 고딕",
    "Apple SD Gothic Neo",
    "Noto Sans CJK KR",
    "Noto Sans KR",
    "NanumGothic",
];
const LIQUID_GLASS_LATIN: &[&str] = &[
    "SF Pro Display",
    "SF Pro Text",
    "Inter",
    "Segoe UI Variable",
    "Segoe UI Variable Text",
    "Segoe UI",
    "Noto Sans",
    "Arial",
];
const LIQUID_GLASS_KOREAN: &[&str] = &[
    "Apple SD Gothic Neo",
    "Pretendard",
    "Malgun Gothic",
    "맑은 고딕",
    "Noto Sans CJK KR",
    "Noto Sans KR",
    "NanumGothic",
];
const SANS_CJK: &[&str] = &[
    "Yu Gothic UI",
    "Microsoft YaHei UI",
    "Malgun Gothic",
    "Noto Sans CJK JP",
];
const MATERIAL_LATIN: &[&str] = &[
    "Roboto",
    "Google Sans",
    "Trebuchet MS",
    "Calibri",
    "Verdana",
    "Segoe UI",
];
const MATERIAL_KOREAN: &[&str] = &[
    "Noto Sans KR",
    "Noto Sans CJK KR",
    "NanumGothic",
    "나눔고딕",
    "Malgun Gothic",
    "맑은 고딕",
    "Apple SD Gothic Neo",
];
const SERIF_LATIN: &[&str] = &[
    "New York",
    "Sitka Text",
    "Georgia",
    "Cambria",
    "Times New Roman",
    "Segoe UI",
];
const SERIF_KOREAN: &[&str] = &[
    "Noto Serif KR",
    "Noto Serif CJK KR",
    "NanumMyeongjo",
    "나눔명조",
    "AppleMyungjo",
    "Batang",
    "바탕",
    "Malgun Gothic",
];
const SERIF_CJK: &[&str] = &[
    "Yu Mincho",
    "MS Mincho",
    "Yu Gothic UI",
    "Microsoft YaHei UI",
];
const MONO_LATIN: &[&str] = &[
    "Cascadia Code",
    "Cascadia Mono",
    "JetBrains Mono",
    "Consolas",
    "Courier New",
    "Segoe UI",
];
const MONO_KOREAN: &[&str] = &[
    "D2Coding",
    "NanumGothicCoding",
    "나눔고딕코딩",
    "Noto Sans Mono CJK KR",
    "Malgun Gothic",
    "맑은 고딕",
];
const MONO_CJK: &[&str] = &[
    "Yu Gothic UI",
    "Microsoft YaHei UI",
    "Noto Sans Mono CJK JP",
];
const EMOJI_CANDIDATES: &[&str] = &[
    "Segoe UI Emoji",
    "Apple Color Emoji",
    "Noto Color Emoji",
    "Segoe UI Symbol",
];

const SUBSTRING_SYMBOL_EXCLUSIONS: &[&str] = &[
    "marlett",
    "wingdings",
    "webdings",
    "mdl2 assets",
    "fluent icons",
    "dingbats",
    "bookshelf symbol",
    "ms outlook",
    "ms reference specialty",
    "mt extra",
];

/// Removes legacy non-Unicode dingbat fonts (`Marlett`, `Wingdings`, `Webdings`, `Symbol`)
/// while preserving `Segoe UI Emoji`, `Segoe UI Symbol`, and `Material Symbols *` icon fonts.
pub(crate) fn purge_symbol_and_dingbat_faces(font_system: &mut FontSystem) {
    let ids_to_remove: Vec<_> = font_system
        .db()
        .faces()
        .filter(|face| {
            face.families.iter().any(|(fam_name, _)| {
                let lower = fam_name.to_ascii_lowercase();
                lower == "symbol"
                    || SUBSTRING_SYMBOL_EXCLUSIONS
                        .iter()
                        .any(|&ex| lower == ex || lower.contains(ex))
            })
        })
        .map(|face| face.id)
        .collect();
    for id in ids_to_remove {
        font_system.db_mut().remove_face(id);
    }
    configure_os_generic_families(font_system);
}

fn configure_os_generic_families(font_system: &mut FontSystem) {
    let available = collect_available_font_families(font_system);
    if let Some(sans) = pick_first(&available, FLUENT_LATIN) {
        font_system.db_mut().set_sans_serif_family(sans);
    }
    if let Some(serif) = pick_first(&available, SERIF_LATIN) {
        font_system.db_mut().set_serif_family(serif);
    }
    if let Some(mono) = pick_first(&available, MONO_LATIN) {
        font_system.db_mut().set_monospace_family(mono);
    }
}

pub(crate) fn collect_available_font_families(font_system: &FontSystem) -> HashSet<String> {
    let mut set = HashSet::new();
    for face in font_system.db().faces() {
        for (fam_name, _) in &face.families {
            set.insert(fam_name.clone());
        }
    }
    set
}

fn pick_first(available: &HashSet<String>, list: &[&str]) -> Option<String> {
    for &c in list {
        if let Some(found) = available.iter().find(|f| f.eq_ignore_ascii_case(c)) {
            return Some(found.clone());
        }
    }
    None
}

fn push_unique_slices(out: &mut Vec<&'static str>, slices: &[&'static [&'static str]]) {
    for slice in slices {
        for &name in *slice {
            if !out
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(name))
            {
                out.push(name);
            }
        }
    }
}

/// Expands a CSS generic family keyword into its platform fallback list, or returns `None` for named families.
pub(crate) fn expand_generic_keyword(token: &str) -> Option<&'static [&'static [&'static str]]> {
    match token.trim().to_ascii_lowercase().as_str() {
        "system-ui" | "ui-sans-serif" | "-apple-system" | "blinkmacsystemfont" => {
            Some(&[SYSTEM_UI_SANS, EMOJI_CANDIDATES])
        }
        "sans-serif" => Some(&[FLUENT_LATIN, FLUENT_KOREAN, SANS_CJK, EMOJI_CANDIDATES]),
        "serif" | "ui-serif" => Some(&[SERIF_LATIN, SERIF_KOREAN, SERIF_CJK, EMOJI_CANDIDATES]),
        "monospace" | "ui-monospace" => {
            Some(&[MONO_LATIN, MONO_KOREAN, MONO_CJK, EMOJI_CANDIDATES])
        }
        "emoji" => Some(&[EMOJI_CANDIDATES]),
        _ => None,
    }
}

/// Expands any [`FontFamilyStack`] into an ordered list of concrete family names in exact CSS declaration order.
pub(crate) fn expand_stack_candidates(stack: FontFamilyStack) -> Vec<&'static str> {
    let mut out = Vec::with_capacity(16);
    match stack {
        FontFamilyStack::FluentUI => {
            push_unique_slices(
                &mut out,
                &[FLUENT_LATIN, FLUENT_KOREAN, SANS_CJK, EMOJI_CANDIDATES],
            );
        }
        FontFamilyStack::MaterialSans => {
            push_unique_slices(
                &mut out,
                &[MATERIAL_LATIN, MATERIAL_KOREAN, SANS_CJK, EMOJI_CANDIDATES],
            );
        }
        FontFamilyStack::LiquidGlassSans => {
            push_unique_slices(
                &mut out,
                &[
                    LIQUID_GLASS_LATIN,
                    LIQUID_GLASS_KOREAN,
                    SANS_CJK,
                    EMOJI_CANDIDATES,
                ],
            );
        }
        FontFamilyStack::Serif => {
            push_unique_slices(
                &mut out,
                &[SERIF_LATIN, SERIF_KOREAN, SERIF_CJK, EMOJI_CANDIDATES],
            );
        }
        FontFamilyStack::CosmicMono => {
            push_unique_slices(
                &mut out,
                &[MONO_LATIN, MONO_KOREAN, MONO_CJK, EMOJI_CANDIDATES],
            );
        }
        FontFamilyStack::Custom(_) => {
            let mut saw_generic = false;
            for &fam in stack.custom_families().unwrap_or(&[]) {
                if let Some(generic_groups) = expand_generic_keyword(fam) {
                    saw_generic = true;
                    push_unique_slices(&mut out, generic_groups);
                } else if !out.iter().any(|e| e.eq_ignore_ascii_case(fam)) {
                    out.push(fam);
                }
            }
            if !saw_generic {
                push_unique_slices(
                    &mut out,
                    &[FLUENT_LATIN, FLUENT_KOREAN, SANS_CJK, EMOJI_CANDIDATES],
                );
            }
        }
    }
    out
}

pub(crate) fn resolve_builtin_multilingual_stacks() -> HashMap<FontFamilyStack, ResolvedFontStack> {
    let mut map = HashMap::with_capacity(8);
    for stack in [
        FontFamilyStack::FluentUI,
        FontFamilyStack::MaterialSans,
        FontFamilyStack::LiquidGlassSans,
        FontFamilyStack::Serif,
        FontFamilyStack::CosmicMono,
    ] {
        map.insert(
            stack,
            ResolvedFontStack {
                candidates: expand_stack_candidates(stack),
            },
        );
    }
    map
}

pub(crate) fn generic_fallback_family(font_family: FontFamilyStack) -> Family<'static> {
    match font_family {
        FontFamilyStack::Serif => Family::Serif,
        FontFamilyStack::CosmicMono => Family::Monospace,
        FontFamilyStack::Custom(_) => {
            let last = font_family
                .custom_families()
                .and_then(|f| f.last().copied())
                .unwrap_or("sans-serif");
            if last.eq_ignore_ascii_case("serif") || last.eq_ignore_ascii_case("ui-serif") {
                Family::Serif
            } else if last.eq_ignore_ascii_case("monospace")
                || last.eq_ignore_ascii_case("ui-monospace")
            {
                Family::Monospace
            } else {
                Family::SansSerif
            }
        }
        _ => Family::SansSerif,
    }
}

//! Embedded KRui starter component packs (`crates/compiler/builtins/*.krui`) and on-demand loader.

use crate::component_slots::compile_schema_defaults;
use crate::css_collect::CollectedStylesheets;
use crate::expr::TemplateExpr;
use crate::html::{parse_html_document, RawHtmlNode};
use crate::html_lexer::extract_style_and_link_blocks;
use crate::CompileError;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum BuiltinPack {
    Actions,
    Communication,
    Containment,
    Navigation,
    Selection,
    TextInputs,
}

const ACTIONS_SRC: &str = include_str!("../builtins/actions.krui");
const COMMUNICATION_SRC: &str = include_str!("../builtins/communication.krui");
const CONTAINMENT_SRC: &str = include_str!("../builtins/containment.krui");
const NAVIGATION_SRC: &str = include_str!("../builtins/navigation.krui");
const SELECTION_SRC: &str = include_str!("../builtins/selection.krui");
const TEXT_INPUTS_SRC: &str = include_str!("../builtins/text_inputs.krui");

const PACK_TABLE: &[(BuiltinPack, &str, &[&str])] = &[
    (
        BuiltinPack::Actions,
        ACTIONS_SRC,
        &[
            "Button",
            "IconButton",
            "SegmentedButton",
            "SegmentItem",
            "SplitButton",
            "Fab",
            "ExtendedFab",
            "FabMenu",
            "FabMenuItem",
        ],
    ),
    (
        BuiltinPack::Communication,
        COMMUNICATION_SRC,
        &[
            "Badge",
            "LinearProgress",
            "CircularProgress",
            "Snackbar",
            "Tooltip",
        ],
    ),
    (
        BuiltinPack::Containment,
        CONTAINMENT_SRC,
        &[
            "Card",
            "Divider",
            "List",
            "ListItem",
            "Carousel",
            "CarouselItem",
            "Dialog",
            "BottomSheet",
            "SideSheet",
        ],
    ),
    (
        BuiltinPack::Navigation,
        NAVIGATION_SRC,
        &[
            "TopAppBar",
            "BottomAppBar",
            "NavigationBar",
            "NavigationBarItem",
            "NavigationRail",
            "NavigationRailItem",
            "NavigationDrawer",
            "NavigationDrawerItem",
            "Tabs",
            "Tab",
            "Menu",
            "MenuItem",
            "Toolbar",
        ],
    ),
    (
        BuiltinPack::Selection,
        SELECTION_SRC,
        &[
            "CheckboxField",
            "RadioField",
            "Switch",
            "SliderField",
            "Chip",
            "SelectField",
            "DatePicker",
            "TimePicker",
        ],
    ),
    (
        BuiltinPack::TextInputs,
        TEXT_INPUTS_SRC,
        &["TextField", "SearchBar", "SearchView"],
    ),
];

/// Parsed template body + pre-compiled prop schema for a built-in component.
pub(crate) type BuiltinTemplateEntry = (Vec<(String, TemplateExpr)>, Vec<RawHtmlNode>);

/// Returns the built-in pack owning component `name`, if `name` is a built-in starter component tag.
pub(crate) fn lookup_builtin_pack(name: &str) -> Option<(BuiltinPack, &'static str)> {
    PACK_TABLE
        .iter()
        .find(|(_, _, names)| names.contains(&name))
        .map(|(pack, src, _)| (*pack, *src))
}

/// Parses `pack`'s `<style>` and `<template>` definitions on demand, registering its CSS into
/// `@layer component` before author stylesheets and returning all templates defined in that pack.
pub(crate) fn load_builtin_pack_on_demand(
    pack: BuiltinPack,
    pack_src: &'static str,
    loaded_packs: &mut HashSet<BuiltinPack>,
    collector: &mut CollectedStylesheets,
    out_templates: &mut HashMap<String, BuiltinTemplateEntry>,
) -> Result<(), CompileError> {
    if loaded_packs.contains(&pack) {
        return Ok(());
    }
    if pack != BuiltinPack::Actions && !loaded_packs.contains(&BuiltinPack::Actions) {
        load_builtin_pack_on_demand(
            BuiltinPack::Actions,
            ACTIONS_SRC,
            loaded_packs,
            collector,
            out_templates,
        )?;
    }
    let (styles, _links, html_text) = extract_style_and_link_blocks(pack_src)?;
    for (css_text, _) in &styles {
        collector.push_builtin_component_css(css_text);
    }
    let (tpls, _roots) = parse_html_document(&html_text)?;
    for tpl in tpls {
        let defaults = compile_schema_defaults(pack_src, &tpl.name, &tpl.default_props)?;
        out_templates.insert(tpl.name, (defaults, tpl.body));
    }
    loaded_packs.insert(pack);
    Ok(())
}

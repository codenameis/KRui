//! # `krui-stylo-bridge`
//!
//! Single authoritative CSS parsing, selector matching, `@media`/`@container`/`@layer`/`@scope`
//! cascade, CSS variable inheritance, and Taffy 0.15 layout style boundary for KRui.
//!
//! ## Architectural Anti-Shotgun-Surgery Contracts
//!
//! 1. **Zero Leaked Stylo Internals**: All `stylo`, `selectors`, and `stylo_taffy` types stay
//!    private inside `krui-stylo-bridge`. Downstream crates interact solely through
//!    [`StyloStyleEngine`], [`StyloNodeId`], [`DomElementState`], [`TaffyLayoutHost`], and
//!    [`run_taffy_layout`].
//! 2. **Zero Raw Pointers in Public API**: Taffy `calc()` callbacks are resolved internally by
//!    [`run_taffy_layout`] while `Arc<ComputedValues>` remains borrowed inside [`StyloStyleEngine`].
//! 3. **Safe Memory & Generational Node Handles**: [`StyloNodeId`] carries `(index, generation)`;
//!    stale handles return `None` or become no-ops, and [`StyloDomMutation::DisposeSubtree`] recycles
//!    freed slots for constant slot counts across 100+ screen switches.
//! 4. **120fps Fast-Path Isolation**: Compositor animations update `PaintSlot` in `krui-core`
//!    without calling [`StyloStyleEngine::restyle`]. Interactive state transitions only dirty Taffy
//!    layout when [`RestyleReport`] marks [`StyloDamage::Relayout`], including `display: none` toggles.
//! 5. **Safety Boundary**: `#![deny(unsafe_code)]` with **0 `unsafe {}` blocks**. Only the 5
//!    trait-mandated `unsafe fn` signatures in `dom_element.rs` carry `#[allow(unsafe_code)]`,
//!    executed strictly single-threaded (`traverse_dom(..., None)`).

#![deny(unsafe_code)]

mod computed_to_gradient;
mod computed_to_layout;
mod computed_to_paint;
mod computed_to_text;
mod computed_to_transform;
mod computed_to_transitions;
mod device;
mod dom_element;
mod dom_mutations;
mod dom_node;
mod dom_store;
mod engine;
mod engine_queries;
mod font_face_extract;
mod hybrid_iter;
mod hybrid_style;
mod keyframes_extract;
mod layout_to_taffy;
mod relative_invalidation;
mod servo_ext;
mod taffy_conv;

pub use engine::StyloStyleEngine;
use krui_style::{
    ColorScheme, DisplayMode, Edges, ObjectFit, OverflowMode, PositionMode, ScrollbarWidth,
    UserSelect,
};
use std::collections::HashMap;
pub use taffy_conv::{run_taffy_layout, run_taffy_layout_standalone, TaffyLayoutHost};

/// Box layout/interaction metadata extracted alongside paint/text styles from Stylo.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxMeta {
    pub display: DisplayMode,
    pub position: PositionMode,
    pub overflow_x: OverflowMode,
    pub overflow_y: OverflowMode,
    pub padding: Edges,
    pub border: Edges,
    pub scrollbar_width: ScrollbarWidth,
    pub z_index: Option<i32>,
    pub visibility_hidden: bool,
    pub pointer_events_none: bool,
    pub object_fit: ObjectFit,
    pub user_select: UserSelect,
}

/// Generational node handle inside [`StyloStyleEngine`]'s mirrored DOM tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StyloNodeId {
    index: u32,
    generation: u32,
}

impl StyloNodeId {
    /// Creates a generational node identifier for internal tests or slot inspection.
    #[must_use]
    pub(crate) const fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }

    /// Arena slot index of this node.
    #[must_use]
    pub const fn index(self) -> u32 {
        self.index
    }

    /// Allocation generation preventing stale-handle use after disposal.
    #[must_use]
    pub const fn generation(self) -> u32 {
        self.generation
    }
}

/// Opaque handle referencing a registered User-Agent or Author stylesheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StyleSheetHandle(pub u64);

/// Cascade origin of a registered stylesheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StyleSheetOrigin {
    /// Built-in UA defaults (`ua_shared.css`, `unstyled.css`).
    UserAgent,
    /// Application or component-scoped author CSS.
    Author,
}

/// Element XML/HTML namespace classification tracked on DOM nodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ElementNamespace {
    /// Standard HTML document namespace (`http://www.w3.org/1999/xhtml`).
    #[default]
    Html,
    /// Inline SVG vector namespace (`http://www.w3.org/2000/svg`).
    Svg,
}

/// Supported CSS pseudo-elements queryable on styled DOM elements.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PseudoElementKind {
    Before,
    After,
    Selection,
    Placeholder,
    Backdrop,
}

/// Pointer input fidelity reported to CSS `@media (pointer)` and `@media (hover)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum PointerKind {
    /// Mouse/trackpad pointer with fine targeting and hover support.
    #[default]
    FineHover,
    /// Touch-first pointer without convenient hover.
    CoarseTouch,
}

/// Media environment configuration applied to Stylo's `Device`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeviceConfig {
    /// Logical viewport width in CSS pixels.
    pub viewport_width: f32,
    /// Logical viewport height in CSS pixels.
    pub viewport_height: f32,
    /// Device pixel ratio (`device_px / css_px`).
    pub dpr: f32,
    /// Active light/dark preference for `@media (prefers-color-scheme)` and `light-dark()`.
    pub color_scheme: ColorScheme,
    /// Primary pointing device fidelity.
    pub pointer: PointerKind,
    /// Whether built-in theme styles are disabled (`krui://preset/unstyled` baseline).
    pub unstyled: bool,
}

impl Default for DeviceConfig {
    fn default() -> Self {
        Self {
            viewport_width: 1280.0,
            viewport_height: 720.0,
            dpr: 1.0,
            color_scheme: ColorScheme::Light,
            pointer: PointerKind::FineHover,
            unstyled: false,
        }
    }
}

/// Platform-neutral element interaction and control state flags mapped internally to Stylo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct DomElementState(u32);

impl DomElementState {
    pub const EMPTY: Self = Self(0);
    pub const HOVER: Self = Self(1 << 0);
    pub const FOCUS: Self = Self(1 << 1);
    pub const FOCUS_VISIBLE: Self = Self(1 << 2);
    pub const FOCUS_WITHIN: Self = Self(1 << 3);
    pub const ACTIVE: Self = Self(1 << 4);
    pub const DISABLED: Self = Self(1 << 5);
    pub const ENABLED: Self = Self(1 << 6);
    pub const CHECKED: Self = Self(1 << 7);
    pub const INDETERMINATE: Self = Self(1 << 8);
    pub const PLACEHOLDER_SHOWN: Self = Self(1 << 9);
    pub const READ_ONLY: Self = Self(1 << 10);
    pub const READ_WRITE: Self = Self(1 << 11);
    pub const REQUIRED: Self = Self(1 << 12);
    pub const VALID: Self = Self(1 << 13);
    pub const INVALID: Self = Self(1 << 14);
    pub const OPEN: Self = Self(1 << 15);
    pub const MODAL: Self = Self(1 << 16);

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    #[must_use]
    pub const fn contains(self, flag: Self) -> bool {
        (self.0 & flag.0) == flag.0
    }
}

/// Non-fatal CSS parse diagnostic collected during stylesheet parsing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CssDiagnostic {
    pub line: u32,
    pub column: u32,
    pub message: String,
}

/// Classification of incremental style changes produced by [`StyloStyleEngine::restyle`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StyloDamage {
    /// Computed style remained identical.
    None,
    /// Only paint-layer properties changed (colors, opacity, border color, shadow, transform).
    Repaint,
    /// Layout-affecting properties or display visibility changed; node must be marked dirty.
    Relayout,
}

/// Per-node damage report returned after an incremental Stylo restyle pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RestyleReport {
    /// Map of nodes that incurred non-empty damage during the last restyle traversal.
    pub nodes: HashMap<StyloNodeId, StyloDamage>,
}

/// Key/value HTML attribute pair synchronized into [`StyloStyleEngine`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ElementAttr {
    pub name: String,
    pub value: String,
}

/// Incremental DOM or interaction mutation streamed from `ViewTree` into [`StyloStyleEngine`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StyloDomMutation {
    /// Appends `child` as the last child of `parent`.
    AppendChild {
        parent: StyloNodeId,
        child: StyloNodeId,
    },
    /// Inserts `child` inside `parent` before `before` (or appends when `None`).
    InsertBefore {
        parent: StyloNodeId,
        child: StyloNodeId,
        before: Option<StyloNodeId>,
    },
    /// Temporarily detaches `node` from its parent without disposing slots.
    DetachNode(StyloNodeId),
    /// Recursively detaches `node` and recycles its entire subtree into the slot free list.
    DisposeSubtree(StyloNodeId),
    /// Updates an element's tag name and namespace (`Html` vs `Svg`).
    SetTag {
        node: StyloNodeId,
        tag_name: String,
        namespace: ElementNamespace,
    },
    /// Replaces an element's normalized CSS class token list.
    SetClasses {
        node: StyloNodeId,
        classes: Vec<String>,
    },
    /// Sets or clears an element's HTML `id` attribute.
    SetId {
        node: StyloNodeId,
        id: Option<String>,
    },
    /// Replaces an element's generic HTML attributes (syncing `"id"`/`"class"` if included).
    SetAttributes {
        node: StyloNodeId,
        attrs: Vec<ElementAttr>,
    },
    /// Parses and sets an inline `style="..."` declaration block on `node`.
    SetInlineStyle { node: StyloNodeId, css: String },
    /// Updates platform-neutral interaction/control state flags on `node`.
    SetState {
        node: StyloNodeId,
        state: DomElementState,
    },
    /// Updates whether a text node contains non-whitespace characters (for `:empty`).
    SetTextNonEmpty { node: StyloNodeId, non_empty: bool },
}

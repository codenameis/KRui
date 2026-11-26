//! Core retained node types (`NodeId`, `ElementNode`, `ViewFragment`, `ClickBinding`).

pub(crate) use crate::view_tree::grapheme::{
    byte_to_char_idx, char_to_byte_idx, delete_active_selection_in_node, next_grapheme_char_idx,
    prev_grapheme_char_idx, snap_char_to_grapheme_boundary, sync_node_text_context,
};
use crate::view_tree::key_routing::{prevent_default_key, KeydownOutcome};
use crate::view_tree::sparse_tables::NodeFlags;
use crate::view_tree::ViewTree;
use krui_a11y::SemanticRole;
use krui_layout::LayoutRect;
use krui_signals::Computed;
use krui_style::{CompiledStyle, TextStyle, UserSelect};
use krui_text::ShapedTextLayout;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

pub(crate) fn compute_node_shaped_key(style: &TextStyle, scale_pct: u32, width_key: u32) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    style.font_size.to_bits().hash(&mut h);
    style.font_weight.hash(&mut h);
    style.font_family.hash(&mut h);
    style.text_align.hash(&mut h);
    style.direction.hash(&mut h);
    style.text_box_trim.hash(&mut h);
    style.text_box_edge.hash(&mut h);
    match style.line_height {
        krui_style::LineHeight::Normal => 0_u8.hash(&mut h),
        krui_style::LineHeight::Factor(f) => {
            1_u8.hash(&mut h);
            f.to_bits().hash(&mut h);
        }
        krui_style::LineHeight::Px(p) => {
            2_u8.hash(&mut h);
            p.to_bits().hash(&mut h);
        }
        krui_style::LineHeight::FontRelative {
            em_factor,
            px_offset,
        } => {
            3_u8.hash(&mut h);
            em_factor.to_bits().hash(&mut h);
            px_offset.to_bits().hash(&mut h);
        }
    }
    for v in style.variations.as_slice() {
        v.tag.hash(&mut h);
        v.value.to_bits().hash(&mut h);
    }
    scale_pct.hash(&mut h);
    width_key.hash(&mut h);
    h.finish()
}

/// Input Method Editor (IME) composition state attached to an `<input>` or `<textarea>` node.
///
/// When IME composition begins, `ViewTree` pins the node as `ime_target` until composition
/// finishes via `Commit`, `FinishComposing`, or `Disabled`. On blur during active `preedit`,
/// KRui auto-commits `preedit` and records `pending_blur_commit` so a subsequent late platform
/// `Commit` either deduplicates identical text or replaces the auto-inserted range.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ImeState {
    /// Whether the platform IME session is currently active for this input.
    pub enabled: bool,
    /// In-progress composition string (`Preedit`), owned by the platform IME.
    pub preedit: String,
    /// Optional byte-offset cursor/clause range `(start_byte, end_byte)` within `preedit`.
    pub preedit_cursor: Option<(usize, usize)>,
    /// Tracks `(start_char, end_char, text)` auto-committed on blur for late platform `Commit` deduplication.
    pub(crate) pending_blur_commit: Option<(usize, usize, String)>,
}

/// Generational identifier for a retained node inside `ViewTree`.
///
/// Accessing a `ViewTree` with a stale `NodeId` (whose slot was unmounted or replaced)
/// safely returns `None` or is ignored without panicking.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeId {
    pub index: u32,
    pub generation: u32,
}

impl NodeId {
    /// Creates a new `NodeId` from `index` and `generation`.
    pub const fn new(index: u32, generation: u32) -> Self {
        Self { index, generation }
    }
}

/// OS mouse cursor shape hint based on CSS `cursor` and hovered widget semantics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HoveredCursorKind {
    #[default]
    Default,
    None,
    Pointer,
    Text,
    Crosshair,
    Move,
    Grab,
    Grabbing,
    EwResize,
    NsResize,
    NeswResize,
    NwseResize,
    ColResize,
    RowResize,
    NotAllowed,
    Wait,
    Progress,
    ZoomIn,
    ZoomOut,
    Help,
    Copy,
    Alias,
    Cell,
    AllScroll,
}

impl HoveredCursorKind {
    /// Maps an explicit CSS `CursorStyle` (`!= Auto`) to [`HoveredCursorKind`].
    #[must_use]
    pub const fn from_css_cursor(cursor: krui_style::CursorStyle) -> Option<Self> {
        use krui_style::CursorStyle as C;
        Some(match cursor {
            C::Auto => return None,
            C::Default => Self::Default,
            C::None => Self::None,
            C::Pointer => Self::Pointer,
            C::Text => Self::Text,
            C::Crosshair => Self::Crosshair,
            C::Move => Self::Move,
            C::Grab => Self::Grab,
            C::Grabbing => Self::Grabbing,
            C::EwResize => Self::EwResize,
            C::NsResize => Self::NsResize,
            C::NeswResize => Self::NeswResize,
            C::NwseResize => Self::NwseResize,
            C::ColResize => Self::ColResize,
            C::RowResize => Self::RowResize,
            C::NotAllowed => Self::NotAllowed,
            C::Wait => Self::Wait,
            C::Progress => Self::Progress,
            C::ZoomIn => Self::ZoomIn,
            C::ZoomOut => Self::ZoomOut,
            C::Help => Self::Help,
            C::Copy => Self::Copy,
            C::Alias => Self::Alias,
            C::Cell => Self::Cell,
            C::AllScroll => Self::AllScroll,
        })
    }
}

pub(crate) type MountFn = Box<dyn FnOnce(&mut ViewTree, Option<NodeId>) -> Vec<NodeId>>;

/// Pattern-neutral click handler trait implemented by MVVM `Command`, plain closures, and `Rc<dyn Fn()>`.
pub trait ClickTarget {
    /// Converts `self` into an executable click callback (`Rc<dyn Fn()>`).
    fn into_click_fn(self) -> Rc<dyn Fn()>;

    /// Optional reactive `Computed<bool>` guard that controls the target node's `:disabled` state.
    fn can_execute_signal(&self) -> Option<Computed<bool>> {
        None
    }
}

impl ClickTarget for Rc<dyn Fn()> {
    fn into_click_fn(self) -> Rc<dyn Fn()> {
        self
    }
}

#[cfg(feature = "preview")]
impl ClickTarget for crate::registry::DynCommand {
    fn into_click_fn(self) -> Rc<dyn Fn()> {
        self.as_rc_fn()
    }

    fn can_execute_signal(&self) -> Option<Computed<bool>> {
        self.can_execute()
    }
}

impl<F> ClickTarget for F
where
    F: Fn() + 'static,
{
    fn into_click_fn(self) -> Rc<dyn Fn()> {
        Rc::new(self)
    }
}

/// Pattern-neutral text input handler trait (`oninput`, `onchange`, `onkeydown`) implemented by
/// `InputCommand`, plain closures `Fn(String)`, and `Rc<dyn Fn(String)>`.
pub trait InputTarget {
    /// Converts `self` into an executable string callback (`Rc<dyn Fn(String)>`).
    fn into_input_fn(self) -> Rc<dyn Fn(String)>;
}

impl InputTarget for Rc<dyn Fn(String)> {
    fn into_input_fn(self) -> Rc<dyn Fn(String)> {
        self
    }
}

#[cfg(feature = "preview")]
impl InputTarget for crate::registry::DynInputHandler {
    fn into_input_fn(self) -> Rc<dyn Fn(String)> {
        self.0
    }
}

impl<F, R> InputTarget for F
where
    F: Fn(String) -> R + 'static,
    R: KeydownOutcome + 'static,
{
    fn into_input_fn(self) -> Rc<dyn Fn(String)> {
        Rc::new(move |val: String| {
            if self(val).into_prevent_default() {
                prevent_default_key();
            }
        })
    }
}

/// Composable view fragment returned by `krui! { ... }` and widget builders.
pub struct ViewFragment {
    mount_fn: MountFn,
}

impl ViewFragment {
    /// Creates a new `ViewFragment` from a mount closure.
    pub fn new(
        mount_fn: impl FnOnce(&mut ViewTree, Option<NodeId>) -> Vec<NodeId> + 'static,
    ) -> Self {
        Self {
            mount_fn: Box::new(mount_fn),
        }
    }

    /// Mounts this fragment into `tree` under `parent`.
    pub fn mount(self, tree: &mut ViewTree, parent: Option<NodeId>) -> Vec<NodeId> {
        (self.mount_fn)(tree, parent)
    }
}

/// Unified click binding target accepting any [`ClickTarget`] (such as an MVVM `Command` with
/// optional `can_execute` `:disabled` state synchronization or a plain callback).
#[derive(Clone)]
pub struct ClickBinding {
    pub(crate) action: Rc<dyn Fn()>,
    pub(crate) can_execute: Option<Computed<bool>>,
}

impl<T: ClickTarget> From<T> for ClickBinding {
    fn from(target: T) -> Self {
        let can_execute = target.can_execute_signal();
        let action = target.into_click_fn();
        Self {
            action,
            can_execute,
        }
    }
}

/// Lean retained DOM element slot. Editing, form control, listener, scroll, canvas, and
/// `<if>`/`<for>` anchor state live in sparse secondary tables (`SparseComponents`) on `ViewTreeInner`.
pub(crate) struct ElementNode {
    pub(crate) id: NodeId,
    pub(crate) flags: NodeFlags,
    pub(crate) role: SemanticRole,
    pub(crate) parent: Option<NodeId>,
    pub(crate) children: Vec<NodeId>,
    pub(crate) taffy_node: Option<taffy::NodeId>,
    pub(crate) _tag: &'static str,
    pub(crate) element_id: Option<String>,
    pub(crate) aria_label: Option<String>,
    pub(crate) style: CompiledStyle,
    pub(crate) world_rect: LayoutRect,
    pub(crate) text: String,
    pub(crate) cached_text_layout: Option<ShapedTextLayout>,
    pub(crate) last_shaped_key: u64,
    #[cfg(feature = "preview")]
    pub(crate) preview_path_key: String,
    #[cfg(feature = "preview")]
    pub(crate) preview_source_span: (u32, u32),
    pub(crate) stylo_node: Option<krui_stylo_bridge::StyloNodeId>,
    pub(crate) text_stylo_node: Option<krui_stylo_bridge::StyloNodeId>,
    pub(crate) last_stylo_state: krui_stylo_bridge::DomElementState,
}

impl ElementNode {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_element(
        id: NodeId,
        parent: Option<NodeId>,
        tag: &'static str,
        element_id: Option<String>,
        aria_label: Option<String>,
        role: SemanticRole,
        is_secret: bool,
        taffy_node: taffy::NodeId,
        style: CompiledStyle,
    ) -> Self {
        let mut flags = NodeFlags::default();
        flags.set(NodeFlags::ALIVE, true);
        flags.set(NodeFlags::IS_SECRET, is_secret);
        flags.set(
            NodeFlags::IS_PROGRESS,
            tag == krui_style::HtmlTag::Progress.as_str(),
        );
        Self {
            id,
            flags,
            role,
            parent,
            children: Vec::new(),
            taffy_node: Some(taffy_node),
            _tag: tag,
            element_id,
            aria_label,
            style,
            world_rect: LayoutRect::default(),
            text: String::new(),
            cached_text_layout: None,
            last_shaped_key: 0,
            #[cfg(feature = "preview")]
            preview_path_key: String::new(),
            #[cfg(feature = "preview")]
            preview_source_span: (0, 0),
            stylo_node: None,
            text_stylo_node: None,
            last_stylo_state: krui_stylo_bridge::DomElementState::EMPTY,
        }
    }

    pub(crate) fn new_anchor(id: NodeId, parent: Option<NodeId>) -> Self {
        let mut flags = NodeFlags::default();
        flags.set(NodeFlags::ALIVE, true);
        flags.set(NodeFlags::IS_ANCHOR, true);
        Self {
            id,
            flags,
            role: SemanticRole::GenericContainer,
            parent,
            children: Vec::new(),
            taffy_node: None,
            _tag: "#anchor",
            element_id: None,
            aria_label: None,
            style: CompiledStyle::default(),
            world_rect: LayoutRect::default(),
            text: String::new(),
            cached_text_layout: None,
            last_shaped_key: 0,
            #[cfg(feature = "preview")]
            preview_path_key: String::new(),
            #[cfg(feature = "preview")]
            preview_source_span: (0, 0),
            stylo_node: None,
            text_stylo_node: None,
            last_stylo_state: krui_stylo_bridge::DomElementState::EMPTY,
        }
    }

    #[inline]
    pub(crate) const fn alive(&self) -> bool {
        self.flags.contains(NodeFlags::ALIVE)
    }

    #[inline]
    pub(crate) const fn is_anchor(&self) -> bool {
        self.flags.contains(NodeFlags::IS_ANCHOR)
    }

    #[inline]
    pub(crate) const fn is_secret(&self) -> bool {
        self.flags.contains(NodeFlags::IS_SECRET)
    }

    #[inline]
    pub(crate) fn set_secret(&mut self, v: bool) {
        self.flags.set(NodeFlags::IS_SECRET, v);
    }

    #[inline]
    pub(crate) const fn is_disabled(&self) -> bool {
        self.flags.contains(NodeFlags::IS_DISABLED)
    }

    #[inline]
    pub(crate) fn set_disabled(&mut self, v: bool) {
        self.flags.set(NodeFlags::IS_DISABLED, v);
    }

    #[inline]
    pub(crate) const fn is_checked(&self) -> bool {
        self.flags.contains(NodeFlags::IS_CHECKED)
    }

    #[inline]
    pub(crate) fn set_checked(&mut self, v: bool) {
        self.flags.set(NodeFlags::IS_CHECKED, v);
    }

    #[inline]
    pub(crate) const fn is_invalid(&self) -> bool {
        self.flags.contains(NodeFlags::IS_INVALID)
    }

    #[inline]
    pub(crate) fn set_invalid(&mut self, v: bool) {
        self.flags.set(NodeFlags::IS_INVALID, v);
    }

    #[inline]
    pub(crate) const fn is_visibility_hidden(&self) -> bool {
        self.flags.contains(NodeFlags::VISIBILITY_HIDDEN)
    }

    #[inline]
    pub(crate) fn set_visibility_hidden(&mut self, v: bool) {
        self.flags.set(NodeFlags::VISIBILITY_HIDDEN, v);
    }

    #[inline]
    pub(crate) const fn is_pointer_events_none(&self) -> bool {
        self.flags.contains(NodeFlags::POINTER_EVENTS_NONE)
    }

    #[inline]
    pub(crate) fn set_pointer_events_none(&mut self, v: bool) {
        self.flags.set(NodeFlags::POINTER_EVENTS_NONE, v);
    }

    #[inline]
    pub(crate) const fn is_synthetic_text_run(&self) -> bool {
        self.flags.contains(NodeFlags::IS_SYNTHETIC_TEXT_RUN)
    }

    #[inline]
    pub(crate) fn set_synthetic_text_run(&mut self, v: bool) {
        self.flags.set(NodeFlags::IS_SYNTHETIC_TEXT_RUN, v);
    }

    #[inline]
    pub(crate) const fn text_dirty(&self) -> bool {
        self.flags.contains(NodeFlags::TEXT_DIRTY)
    }

    #[inline]
    pub(crate) fn set_text_dirty(&mut self, v: bool) {
        self.flags.set(NodeFlags::TEXT_DIRTY, v);
    }

    #[inline]
    pub(crate) const fn has_dynamic_text_binding(&self) -> bool {
        self.flags.contains(NodeFlags::HAS_DYNAMIC_TEXT_BINDING)
    }

    #[inline]
    pub(crate) fn set_has_dynamic_text_binding(&mut self, v: bool) {
        self.flags.set(NodeFlags::HAS_DYNAMIC_TEXT_BINDING, v);
    }

    pub(crate) fn clear_for_unmount(&mut self) -> Option<taffy::NodeId> {
        self.flags.set(NodeFlags::ALIVE, false);
        #[cfg(feature = "preview")]
        self.preview_path_key.clear();
        self.text.clear();
        self.cached_text_layout = None;
        self.children.clear();
        self.stylo_node = None;
        self.text_stylo_node = None;
        self.last_stylo_state = krui_stylo_bridge::DomElementState::EMPTY;
        self.taffy_node.take()
    }
}

pub(crate) fn resolve_tag_semantic_role(
    tag: &'static str,
    is_button: bool,
    is_input: bool,
    is_canvas: bool,
    is_secret: bool,
) -> SemanticRole {
    if is_secret {
        SemanticRole::PasswordInput
    } else if is_input {
        SemanticRole::TextInput
    } else if is_button {
        SemanticRole::Button
    } else if is_canvas {
        SemanticRole::Canvas
    } else {
        match tag {
            "textarea" => SemanticRole::MultilineTextInput,
            "form" => SemanticRole::Form,
            "select" => SemanticRole::ComboBox,
            "a" => SemanticRole::Link,
            "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => SemanticRole::Heading,
            "span" | "p" | "label" | "b" | "strong" | "em" | "code" => SemanticRole::Label,
            "ul" => SemanticRole::List,
            "li" => SemanticRole::ListItem,
            _ => SemanticRole::GenericContainer,
        }
    }
}

impl crate::view_tree::ViewTreeInner {
    pub(crate) fn allocate_slot(&mut self) -> (u32, u32) {
        if let Some(free_idx) = self.free_nodes.pop() {
            (free_idx, self.generations[free_idx as usize])
        } else {
            let idx = self.nodes.len() as u32;
            if (idx as usize) >= self.generations.len() {
                self.generations.push(0);
            }
            (idx, self.generations[idx as usize])
        }
    }
}

/// Origin of the current focus assignment (`Keyboard` triggers `:focus-visible`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FocusOrigin {
    #[default]
    Programmatic,
    Pointer,
    Keyboard,
}

pub(crate) fn is_node_text_selectable(node: &ElementNode) -> bool {
    match node.style.paint.user_select {
        UserSelect::None => false,
        UserSelect::Text | UserSelect::All => true,
        UserSelect::Auto => node.role.is_text_editable(),
    }
}

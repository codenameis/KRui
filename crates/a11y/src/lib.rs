//! # `krui-a11y`
//!
//! AccessKit semantic tree builder, keyboard focus navigator, and secret-field hygiene (Section 5 & Section 6).

#![forbid(unsafe_code)]

use accesskit::{Action, Node, NodeId as AkNodeId, Rect, Role, Toggled, Tree, TreeUpdate};
use krui_layout::LayoutRect;

/// Semantic role for a KRui element in the accessibility tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticRole {
    Window,
    GenericContainer,
    Form,
    Heading,
    Label,
    Button,
    TextInput,
    PasswordInput,
    MultilineTextInput,
    Checkbox,
    RadioButton,
    Slider,
    ComboBox,
    List,
    ListItem,
    Canvas,
    Link,
}

impl SemanticRole {
    pub const fn to_accesskit_role(self) -> Role {
        match self {
            Self::Window => Role::Window,
            Self::GenericContainer => Role::GenericContainer,
            Self::Form => Role::Form,
            Self::Heading => Role::Heading,
            Self::Label => Role::Label,
            Self::Button => Role::Button,
            Self::TextInput => Role::TextInput,
            Self::PasswordInput => Role::PasswordInput,
            Self::MultilineTextInput => Role::MultilineTextInput,
            Self::Checkbox => Role::CheckBox,
            Self::RadioButton => Role::RadioButton,
            Self::Slider => Role::Slider,
            Self::ComboBox => Role::ComboBox,
            Self::List => Role::List,
            Self::ListItem => Role::ListItem,
            Self::Canvas => Role::Canvas,
            Self::Link => Role::Link,
        }
    }

    pub const fn is_focusable(self) -> bool {
        matches!(
            self,
            Self::Button
                | Self::Link
                | Self::TextInput
                | Self::PasswordInput
                | Self::MultilineTextInput
                | Self::Checkbox
                | Self::RadioButton
                | Self::Slider
                | Self::ComboBox
        )
    }

    pub const fn is_text_editable(self) -> bool {
        matches!(
            self,
            Self::TextInput | Self::PasswordInput | Self::MultilineTextInput
        )
    }

    pub const fn is_single_line_input(self) -> bool {
        matches!(self, Self::TextInput | Self::PasswordInput)
    }
}

/// Input descriptor for building a single AccessKit node.
#[derive(Clone, Debug)]
pub struct A11yNodeDescriptor {
    pub id: u64,
    pub role: SemanticRole,
    pub label: Option<String>,
    pub value: Option<String>,
    /// Optional hyperlink URL exposed on `SemanticRole::Link` nodes.
    pub url: Option<String>,
    /// When `true` (or when `role == SemanticRole::PasswordInput`), the exported AccessKit
    /// node masks its value (`"••••••••"`) and never exposes raw secret text to external processes (Section 6).
    pub is_secret: bool,
    /// When `true`, the control is excluded from `Tab` / `Shift+Tab` traversal and marked disabled in AccessKit.
    pub is_disabled: bool,
    /// Optional checked state for `Checkbox` / `RadioButton` controls.
    pub is_checked: Option<bool>,
    pub bounds: LayoutRect,
    pub children: Vec<u64>,
}

/// Manages keyboard focus navigation (`Tab` / `Shift+Tab`) and builds `accesskit::TreeUpdate` snapshots.
#[derive(Clone, Debug, Default)]
pub struct AccessibilityEngine {
    focusable_order: Vec<u64>,
    focused_id: Option<u64>,
}

impl AccessibilityEngine {
    pub const ROOT_A11Y_ID: u64 = 1;
    pub const MASKED_SECRET_PLACEHOLDER: &'static str = "••••••••";

    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a focusable node ID in the tab order at node creation time.
    pub fn register_focusable(&mut self, node_id: u64) {
        if !self.focusable_order.contains(&node_id) {
            self.focusable_order.push(node_id);
        }
    }

    /// Replaces the current `Tab` / `Shift+Tab` focusable traversal order with `order`.
    pub fn sync_focusable_order(&mut self, order: Vec<u64>) {
        self.focusable_order = order;
    }

    /// Unregisters a focusable node ID when it is unmounted from the view tree.
    pub fn unregister_focusable(&mut self, node_id: u64) {
        self.focusable_order.retain(|&id| id != node_id);
        if self.focused_id == Some(node_id) {
            self.focused_id = None;
        }
    }

    /// Returns the currently focused node ID, if any.
    pub fn focused_id(&self) -> Option<u64> {
        self.focused_id
    }

    /// Explicitly sets keyboard focus to `node_id`.
    pub fn set_focus(&mut self, node_id: Option<u64>) {
        self.focused_id = node_id;
    }

    /// Advances keyboard focus to the next focusable widget (`Tab`).
    pub fn focus_next(&mut self) -> Option<u64> {
        if self.focusable_order.is_empty() {
            self.focused_id = None;
            return None;
        }
        let next_idx = match self
            .focused_id
            .and_then(|id| self.focusable_order.iter().position(|&x| x == id))
        {
            Some(idx) => (idx + 1) % self.focusable_order.len(),
            None => 0,
        };
        let next_id = self.focusable_order[next_idx];
        self.focused_id = Some(next_id);
        Some(next_id)
    }

    /// Moves keyboard focus to the previous focusable widget (`Shift+Tab`).
    pub fn focus_previous(&mut self) -> Option<u64> {
        if self.focusable_order.is_empty() {
            self.focused_id = None;
            return None;
        }
        let prev_idx = match self
            .focused_id
            .and_then(|id| self.focusable_order.iter().position(|&x| x == id))
        {
            Some(0) | None => self.focusable_order.len() - 1,
            Some(idx) => idx - 1,
        };
        let prev_id = self.focusable_order[prev_idx];
        self.focused_id = Some(prev_id);
        Some(prev_id)
    }

    /// Builds an `accesskit::TreeUpdate` from the current view descriptors while enforcing
    /// Section 6 secret/password masking.
    pub fn build_tree_update(
        &mut self,
        descriptors: &[A11yNodeDescriptor],
        root_id: u64,
    ) -> TreeUpdate {
        self.focusable_order.clear();
        let mut nodes = Vec::with_capacity(descriptors.len());

        for desc in descriptors {
            if desc.role.is_focusable() && !desc.is_disabled {
                self.focusable_order.push(desc.id);
            }
            nodes.push((AkNodeId(desc.id), Self::build_single_node(desc)));
        }

        let focus_ak_id = self
            .focused_id
            .filter(|id| self.focusable_order.contains(id))
            .unwrap_or(root_id);

        TreeUpdate {
            nodes,
            tree: Some(Tree::new(AkNodeId(root_id))),
            focus: AkNodeId(focus_ak_id),
        }
    }

    fn build_single_node(desc: &A11yNodeDescriptor) -> Node {
        let mut node = Node::new(desc.role.to_accesskit_role());
        node.set_bounds(Rect {
            x0: desc.bounds.x as f64,
            y0: desc.bounds.y as f64,
            x1: (desc.bounds.x + desc.bounds.width) as f64,
            y1: (desc.bounds.y + desc.bounds.height) as f64,
        });
        let mask = desc.is_secret || desc.role == SemanticRole::PasswordInput;
        if let Some(label) = &desc.label {
            node.set_label(if mask {
                "Password input"
            } else {
                label.as_str()
            });
        }
        if let Some(val) = &desc.value {
            node.set_value(if mask {
                Self::MASKED_SECRET_PLACEHOLDER
            } else {
                val.as_str()
            });
        }
        if let Some(url) = &desc.url {
            if !mask && !url.is_empty() {
                node.set_url(url.as_str());
            }
        }
        if let Some(checked) = desc.is_checked {
            node.set_toggled(if checked {
                Toggled::True
            } else {
                Toggled::False
            });
        }
        if desc.is_disabled {
            node.set_disabled();
        } else {
            Self::attach_node_actions(&mut node, desc);
        }
        node.set_children(
            desc.children
                .iter()
                .map(|&cid| AkNodeId(cid))
                .collect::<Vec<_>>(),
        );
        node
    }

    fn attach_node_actions(node: &mut Node, desc: &A11yNodeDescriptor) {
        if desc.role.is_focusable() {
            node.add_action(Action::Focus);
        }
        if matches!(
            desc.role,
            SemanticRole::Button
                | SemanticRole::Link
                | SemanticRole::Checkbox
                | SemanticRole::RadioButton
                | SemanticRole::ComboBox
        ) {
            node.add_action(Action::Click);
        }
        if desc.role.is_text_editable()
            || matches!(desc.role, SemanticRole::Slider | SemanticRole::ComboBox)
        {
            node.add_action(Action::SetValue);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_descriptor(
        id: u64,
        role: SemanticRole,
        label: Option<&str>,
        value: Option<&str>,
        is_secret: bool,
        children: Vec<u64>,
    ) -> A11yNodeDescriptor {
        A11yNodeDescriptor {
            id,
            role,
            label: label.map(str::to_string),
            value: value.map(str::to_string),
            url: None,
            is_secret,
            is_disabled: false,
            is_checked: None,
            bounds: LayoutRect {
                x: 0.0,
                y: 0.0,
                width: 200.0,
                height: 36.0,
            },
            children,
        }
    }

    #[test]
    fn masks_secret_and_password_fields_in_accesskit_tree() {
        let mut engine = AccessibilityEngine::new();
        let descriptors = vec![
            test_descriptor(
                1,
                SemanticRole::Window,
                Some("Main Window"),
                None,
                false,
                vec![2, 3],
            ),
            test_descriptor(
                2,
                SemanticRole::PasswordInput,
                Some("super_secret_hunter2"),
                Some("super_secret_hunter2"),
                true,
                vec![],
            ),
            test_descriptor(3, SemanticRole::Button, Some("로그인"), None, false, vec![]),
        ];

        let update = engine.build_tree_update(&descriptors, 1);
        let (_, pwd_node) = &update.nodes[1];
        assert_eq!(
            pwd_node.value(),
            Some(AccessibilityEngine::MASKED_SECRET_PLACEHOLDER)
        );
        assert_ne!(pwd_node.label(), Some("super_secret_hunter2"));

        assert_eq!(engine.focus_next(), Some(2));
        assert_eq!(engine.focus_next(), Some(3));
        assert_eq!(engine.focus_next(), Some(2));
        assert_eq!(engine.focus_previous(), Some(3));
    }
}

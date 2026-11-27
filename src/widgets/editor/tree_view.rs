//! 대규모 씬 계층용 가상화 `TreeViewController`(드래그앤드롭 재부모화 + 순환 참조 차단).

use krui_signals::{batch, Signal};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// 트리 뷰에 등록하는 단일 계층 노드 데이터.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeNodeData {
    /// 고유 노드/엔티티 ID.
    pub id: u64,
    /// 부모 ID (`None`이면 루트 레벨).
    pub parent: Option<u64>,
    /// 표시 이름.
    pub label: String,
    /// 자식 펼침 여부.
    pub expanded: bool,
}

/// 가상화 뷰포트 창에 렌더링할 평탄화된 가시 행 정보.
#[derive(Clone, Debug, PartialEq)]
pub struct VisibleTreeRow {
    /// 노드 ID.
    pub id: u64,
    /// 표시 이름.
    pub label: String,
    /// 계층 깊이(`0` = 루트).
    pub depth: usize,
    /// 자식 보유 여부.
    pub has_children: bool,
    /// 현재 펼침 상태.
    pub expanded: bool,
    /// 현재 선택 여부.
    pub selected: bool,
    /// 전체 가상 리스트 내 Y 오프셋(px).
    pub top_px: f32,
}

/// 드래그앤드롭 목표 위치.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropPlacement {
    /// 대상 형제 앞.
    Before,
    /// 대상의 자식으로 삽입.
    Inside,
    /// 대상 형제 뒤.
    After,
}

#[derive(Default)]
struct TreeStore {
    nodes: Vec<TreeNodeData>,
    dragging_id: Option<u64>,
    drop_target: Option<(u64, DropPlacement)>,
}

/// 수천 개 엔티티를 일정 O(visible) 윈도우로 렌더링하고 재부모화를 검증하는 트리 컨트롤러.
#[derive(Clone)]
pub struct TreeViewController {
    store: Rc<RefCell<TreeStore>>,
    /// 현재 선택된 노드 ID.
    pub selected_id: Signal<Option<u64>>,
    /// 현재 펼침 상태를 반영한 가시 행 총 개수.
    pub visible_count: Signal<usize>,
    /// 트리 구조 변경 리비전.
    pub revision: Signal<u64>,
}

impl Default for TreeViewController {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeViewController {
    /// 빈 `TreeViewController`를 생성합니다.
    pub fn new() -> Self {
        Self {
            store: Rc::new(RefCell::new(TreeStore::default())),
            selected_id: Signal::new(None),
            visible_count: Signal::new(0),
            revision: Signal::new(0),
        }
    }

    /// 전체 노드 목록을 교체하고 가시 행 개수를 갱신합니다.
    pub fn set_nodes(&self, nodes: Vec<TreeNodeData>) {
        self.store.borrow_mut().nodes = nodes;
        self.refresh_meta();
    }

    /// 현재 보유 중인 전체 노드 스냅샷을 반환합니다.
    pub fn nodes(&self) -> Vec<TreeNodeData> {
        self.store.borrow().nodes.clone()
    }

    /// 특정 노드를 선택합니다.
    pub fn select(&self, id: Option<u64>) {
        self.selected_id.set(id);
    }

    /// `id`의 자식 펼침/접힘을 토글합니다.
    pub fn toggle_expand(&self, id: u64) -> bool {
        let changed = {
            let mut st = self.store.borrow_mut();
            if let Some(node) = st.nodes.iter_mut().find(|n| n.id == id) {
                node.expanded = !node.expanded;
                true
            } else {
                false
            }
        };
        if changed {
            self.refresh_meta();
        }
        changed
    }

    /// 스크롤 위치 `scroll_y`와 화면 높이 `viewport_h`에 해당하는 가시 행과 전체 콘텐츠 높이를 반환합니다.
    pub fn visible_window(
        &self,
        scroll_y: f32,
        viewport_h: f32,
        row_h: f32,
    ) -> (Vec<VisibleTreeRow>, f32) {
        let flat = self.flatten_visible();
        let rh = if row_h.is_finite() {
            row_h.max(1.0)
        } else {
            24.0
        };
        let total_h = flat.len() as f32 * rh;
        let safe_scroll = if scroll_y.is_finite() {
            scroll_y.max(0.0)
        } else {
            0.0
        };
        let safe_vp = if viewport_h.is_finite() {
            viewport_h.clamp(0.0, 100_000.0)
        } else {
            1_024.0
        };
        let first_f = (safe_scroll / rh).floor();
        let first = if first_f >= flat.len() as f32 {
            flat.len()
        } else {
            first_f as usize
        };
        let max_rows = ((safe_vp / rh).ceil() as usize).saturating_add(2);
        let end = first.saturating_add(max_rows).min(flat.len());
        let rows = if first >= end {
            Vec::new()
        } else {
            flat[first..end]
                .iter()
                .enumerate()
                .map(|(i, row)| {
                    let mut out = row.clone();
                    out.top_px = (first + i) as f32 * rh;
                    out
                })
                .collect()
        };
        (rows, total_h)
    }

    /// `parent` 아래 직속 자식 ID 순서를 반환합니다.
    pub fn children_of(&self, parent: Option<u64>) -> Vec<u64> {
        self.store
            .borrow()
            .nodes
            .iter()
            .filter(|n| n.parent == parent)
            .map(|n| n.id)
            .collect()
    }

    /// 드래그 재정렬/재부모화를 시작합니다.
    pub fn begin_drag(&self, id: u64) {
        let mut st = self.store.borrow_mut();
        st.dragging_id = Some(id);
        st.drop_target = None;
    }

    /// 드롭 대상과 위치를 설정합니다 (`Inside`/`Before`/`After`).
    pub fn set_drop_target(&self, target_id: u64, placement: DropPlacement) {
        self.store.borrow_mut().drop_target = Some((target_id, placement));
    }

    /// 진행 중인 드래그 상태를 취소합니다.
    pub fn cancel_drag(&self) {
        let mut st = self.store.borrow_mut();
        st.dragging_id = None;
        st.drop_target = None;
    }

    /// 드롭을 확정합니다. 자기 자신이나 후손 노드로의 순환 재부모화는 거부하고 `false`를 반환합니다.
    pub fn commit_drop(&self) -> bool {
        let committed = {
            let mut st = self.store.borrow_mut();
            apply_tree_drop(&mut st)
        };
        if committed {
            self.refresh_meta();
        }
        committed
    }

    fn flatten_visible(&self) -> Vec<VisibleTreeRow> {
        let st = self.store.borrow();
        let selected = self.selected_id.get_untracked();
        let mut by_parent: HashMap<Option<u64>, Vec<&TreeNodeData>> = HashMap::new();
        let ids: HashSet<u64> = st.nodes.iter().map(|n| n.id).collect();
        for node in &st.nodes {
            let p = node.parent.filter(|pid| ids.contains(pid));
            by_parent.entry(p).or_default().push(node);
        }
        let mut out = Vec::new();
        let mut visited = HashSet::with_capacity(st.nodes.len());
        collect_visible_dfs(&by_parent, None, 0, selected, &mut visited, &mut out);
        out
    }

    fn refresh_meta(&self) {
        let count = self.flatten_visible().len();
        let vc = self.visible_count;
        let rev = self.revision;
        let next_rev = rev.get_untracked().wrapping_add(1);
        batch(move || {
            vc.set(count);
            rev.set(next_rev);
        });
    }
}

const MAX_TREE_VIEW_DEPTH: usize = 256;

fn collect_visible_dfs(
    by_parent: &HashMap<Option<u64>, Vec<&TreeNodeData>>,
    parent: Option<u64>,
    depth: usize,
    selected: Option<u64>,
    visited: &mut HashSet<u64>,
    out: &mut Vec<VisibleTreeRow>,
) {
    if depth > MAX_TREE_VIEW_DEPTH {
        return;
    }
    let Some(children) = by_parent.get(&parent) else {
        return;
    };
    for node in children {
        if !visited.insert(node.id) {
            continue;
        }
        let has_children = by_parent.get(&Some(node.id)).is_some_and(|c| !c.is_empty());
        out.push(VisibleTreeRow {
            id: node.id,
            label: node.label.clone(),
            depth,
            has_children,
            expanded: node.expanded,
            selected: selected == Some(node.id),
            top_px: 0.0,
        });
        if node.expanded && has_children {
            collect_visible_dfs(by_parent, Some(node.id), depth + 1, selected, visited, out);
        }
    }
}

fn apply_tree_drop(st: &mut TreeStore) -> bool {
    let (Some(drag_id), Some((target_id, placement))) =
        (st.dragging_id.take(), st.drop_target.take())
    else {
        return false;
    };
    if drag_id == target_id {
        return false;
    }
    let Some(drag_pos) = st.nodes.iter().position(|n| n.id == drag_id) else {
        return false;
    };
    let Some(target_parent) = st
        .nodes
        .iter()
        .find(|n| n.id == target_id)
        .map(|n| n.parent)
    else {
        return false;
    };
    let new_parent = match placement {
        DropPlacement::Inside => Some(target_id),
        DropPlacement::Before | DropPlacement::After => target_parent,
    };
    if let Some(pid) = new_parent {
        if is_descendant_or_self(&st.nodes, drag_id, pid) {
            return false;
        }
    }
    let mut moved = st.nodes.remove(drag_pos);
    moved.parent = new_parent;
    let insert_idx = match placement {
        DropPlacement::Before => st
            .nodes
            .iter()
            .position(|n| n.id == target_id)
            .unwrap_or(st.nodes.len()),
        DropPlacement::After => st
            .nodes
            .iter()
            .position(|n| n.id == target_id)
            .map_or(st.nodes.len(), |i| i + 1),
        DropPlacement::Inside => st.nodes.len(),
    };
    st.nodes.insert(insert_idx, moved);
    if let Some(pid) = new_parent {
        if let Some(parent_node) = st.nodes.iter_mut().find(|n| n.id == pid) {
            parent_node.expanded = true;
        }
    }
    true
}

fn is_descendant_or_self(nodes: &[TreeNodeData], ancestor_id: u64, mut probe_id: u64) -> bool {
    for _ in 0..=nodes.len() {
        if probe_id == ancestor_id {
            return true;
        }
        let Some(next) = nodes
            .iter()
            .find(|n| n.id == probe_id)
            .and_then(|n| n.parent)
        else {
            return false;
        };
        probe_id = next;
    }
    true
}

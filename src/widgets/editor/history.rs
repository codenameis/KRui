//! 3D 에디터 · 캔버스 공용 바운디드 `Undo/Redo` 이력 스택 (`UndoHistory<T>`).

use krui_signals::{batch, Signal};
use std::cell::RefCell;
use std::rc::Rc;

/// 단일 편집 스냅샷 기록.
#[derive(Clone, Debug, PartialEq)]
pub struct UndoEntry<T> {
    /// 사용자 표시용 작업 이름 (예: `"Move Entity"`).
    pub label: String,
    /// 동일 키 연속 드래그를 하나의 실행 취소 단위로 병합할 때 사용하는 선택적 병합 키.
    pub merge_key: Option<u64>,
    /// 변경 이전 상태 스냅샷.
    pub before: T,
    /// 변경 이후 상태 스냅샷.
    pub after: T,
}

#[derive(Debug)]
struct HistoryCore<T> {
    capacity: usize,
    undo: Vec<UndoEntry<T>>,
    redo: Vec<UndoEntry<T>>,
}

/// 드래그 병합(`push_coalesced`)과 반응형 UI 신호를 지원하는 공용 `Undo/Redo` 컨트롤러.
#[derive(Clone)]
pub struct UndoHistory<T> {
    core: Rc<RefCell<HistoryCore<T>>>,
    /// 실행 취소 가능 여부 신호.
    pub can_undo: Signal<bool>,
    /// 다시 실행 가능 여부 신호.
    pub can_redo: Signal<bool>,
    /// 다음 실행 취소 작업 이름 신호.
    pub undo_label: Signal<String>,
    /// 다음 다시 실행 작업 이름 신호.
    pub redo_label: Signal<String>,
    /// 이력이 바뀔 때마다 증가하는 리비전 신호.
    pub revision: Signal<u64>,
}

impl<T: Clone + PartialEq + 'static> UndoHistory<T> {
    /// 최대 `capacity` 단계까지 보관하는 새 `UndoHistory`를 만듭니다.
    pub fn new(capacity: usize) -> Self {
        Self {
            core: Rc::new(RefCell::new(HistoryCore {
                capacity: capacity.max(1),
                undo: Vec::new(),
                redo: Vec::new(),
            })),
            can_undo: Signal::new(false),
            can_redo: Signal::new(false),
            undo_label: Signal::new(String::new()),
            redo_label: Signal::new(String::new()),
            revision: Signal::new(0),
        }
    }

    /// 새 편집 단계를 기록합니다. `before == after`이면 무시합니다.
    pub fn push(&self, label: impl Into<String>, before: T, after: T) {
        self.push_internal(label.into(), None, before, after);
    }

    /// 같은 `merge_key`가 최상단에 있으면 `before`를 유지한 채 `after`만 갱신해 연속 드래그를 하나로 묶습니다.
    pub fn push_coalesced(&self, label: impl Into<String>, merge_key: u64, before: T, after: T) {
        self.push_internal(label.into(), Some(merge_key), before, after);
    }

    fn push_internal(&self, label: String, merge_key: Option<u64>, before: T, after: T) {
        if before == after {
            return;
        }
        {
            let mut core = self.core.borrow_mut();
            core.redo.clear();
            let merged = merge_key.is_some()
                && core.undo.last_mut().is_some_and(|top| {
                    if top.merge_key == merge_key {
                        top.after = after.clone();
                        top.label = label.clone();
                        true
                    } else {
                        false
                    }
                });
            if !merged {
                if core.undo.len() >= core.capacity {
                    let excess = core.undo.len() + 1 - core.capacity;
                    core.undo.drain(0..excess);
                }
                core.undo.push(UndoEntry {
                    label,
                    merge_key,
                    before,
                    after,
                });
            }
        }
        self.sync_signals();
    }

    /// 가장 최근 편집을 되돌리고 복원할 `before` 스냅샷을 반환합니다.
    pub fn undo(&self) -> Option<T> {
        let out = {
            let mut core = self.core.borrow_mut();
            let entry = core.undo.pop()?;
            let prev = entry.before.clone();
            core.redo.push(entry);
            prev
        };
        self.sync_signals();
        Some(out)
    }

    /// 가장 최근 되돌린 편집을 다시 적용하고 `after` 스냅샷을 반환합니다.
    pub fn redo(&self) -> Option<T> {
        let out = {
            let mut core = self.core.borrow_mut();
            let entry = core.redo.pop()?;
            let next = entry.after.clone();
            core.undo.push(entry);
            next
        };
        self.sync_signals();
        Some(out)
    }

    /// 현재 남아 있는 실행 취소 스택 깊이를 반환합니다.
    pub fn undo_len(&self) -> usize {
        self.core.borrow().undo.len()
    }

    /// 현재 남아 있는 다시 실행 스택 깊이를 반환합니다.
    pub fn redo_len(&self) -> usize {
        self.core.borrow().redo.len()
    }

    /// 모든 실행 취소/다시 실행 기록을 비웁니다.
    pub fn clear(&self) {
        {
            let mut core = self.core.borrow_mut();
            core.undo.clear();
            core.redo.clear();
        }
        self.sync_signals();
    }

    fn sync_signals(&self) {
        let (has_u, has_r, u_lbl, r_lbl) = {
            let core = self.core.borrow();
            (
                !core.undo.is_empty(),
                !core.redo.is_empty(),
                core.undo
                    .last()
                    .map_or_else(String::new, |e| e.label.clone()),
                core.redo
                    .last()
                    .map_or_else(String::new, |e| e.label.clone()),
            )
        };
        let can_u = self.can_undo;
        let can_r = self.can_redo;
        let ul = self.undo_label;
        let rl = self.redo_label;
        let rev = self.revision;
        let next_rev = rev.get_untracked().wrapping_add(1);
        batch(move || {
            can_u.set(has_u);
            can_r.set(has_r);
            ul.set(u_lbl);
            rl.set(r_lbl);
            rev.set(next_rev);
        });
    }
}

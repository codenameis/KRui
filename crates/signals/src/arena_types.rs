//! Generational IDs, diagnostic `RuntimeStats`, and internal arena slot types for `krui-signals`.

use std::any::Any;
use std::rc::Rc;

/// Generational identifier for a signal node in the thread-local reactive arena.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SignalId {
    pub index: u32,
    pub generation: u32,
}

/// Generational identifier for a subscriber (`Computed` or `Effect`) in the reactive arena.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct SubscriberId {
    pub index: u32,
    pub generation: u32,
}

/// Type alias for a `Computed` subscriber identifier.
pub type ComputedId = SubscriberId;

/// Type alias for an `Effect` subscriber identifier.
pub type EffectId = SubscriberId;

/// Generational identifier for a reactive ownership `Scope`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct ScopeId {
    pub index: u32,
    pub generation: u32,
}

impl ScopeId {
    /// The permanent root scope (`index: 0, generation: 0`).
    pub const ROOT: Self = Self {
        index: 0,
        generation: 0,
    };
}

/// Diagnostic snapshot of live reactive arena counts (used for memory/leak regression tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct RuntimeStats {
    pub live_signals: usize,
    pub live_computed: usize,
    pub live_subscribers: usize,
    pub live_effects: usize,
    pub live_scopes: usize,
    pub total_subscriber_entries: usize,
    pub live_async_pollers: usize,
    /// Number of unbounded retained fallback/tombstone entries for disposed signals (always `0`).
    pub retained_disposed_fallbacks: usize,
}

/// Push-pull invalidation state for a `Computed` subscriber node.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ComputedState {
    Clean,
    Check,
    Dirty,
    Computing,
}

thread_local! {
    static EMPTY_ANY_RC: Rc<dyn Any> = Rc::new(());
}

pub(crate) fn empty_any_rc() -> Rc<dyn Any> {
    EMPTY_ANY_RC.with(Rc::clone)
}

pub(crate) struct SignalSlot {
    pub(crate) generation: u32,
    pub(crate) alive: bool,
    pub(crate) value: Rc<dyn Any>,
    pub(crate) initial_value: Rc<dyn Any>,
    pub(crate) producer: Option<SubscriberId>,
    pub(crate) subscribers: Vec<SubscriberId>,
    pub(crate) last_subscribed_run: Option<(SubscriberId, u64)>,
    pub(crate) is_frame_driven: bool,
    pub(crate) borrow_depth: usize,
    pub(crate) deferred_notify: bool,
    pub(crate) updating_in_place: bool,
    pub(crate) overridden_during_update: bool,
}

impl SignalSlot {
    pub(crate) fn dispose(&mut self) -> (u32, bool) {
        self.alive = false;
        self.producer = None;
        self.subscribers.clear();
        self.last_subscribed_run = None;
        self.borrow_depth = 0;
        self.deferred_notify = false;
        self.updating_in_place = false;
        self.value = empty_any_rc();
        self.initial_value = empty_any_rc();
        let (next_gen, saturated) = crate::session_hub::bump_preserving_tag(self.generation);
        self.generation = next_gen;
        (next_gen, saturated)
    }
}

pub(crate) struct SubscriberSlot {
    pub(crate) generation: u32,
    pub(crate) alive: bool,
    pub(crate) is_effect: bool,
    pub(crate) state: ComputedState,
    pub(crate) depth: u32,
    pub(crate) owner_scope: ScopeId,
    pub(crate) run_scope: Option<ScopeId>,
    pub(crate) produced_signal: Option<SignalId>,
    pub(crate) dependencies: Vec<SignalId>,
    pub(crate) callback: Option<Rc<dyn Fn()>>,
}

pub(crate) struct ScopeSlot {
    pub(crate) generation: u32,
    pub(crate) alive: bool,
    pub(crate) parent: Option<ScopeId>,
    pub(crate) owned_signals: Vec<SignalId>,
    pub(crate) owned_subscribers: Vec<SubscriberId>,
    pub(crate) owned_children: Vec<ScopeId>,
}

impl ScopeSlot {
    pub(crate) fn new(generation: u32, parent: Option<ScopeId>) -> Self {
        Self {
            generation,
            alive: true,
            parent,
            owned_signals: Vec::new(),
            owned_subscribers: Vec::new(),
            owned_children: Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct TrackingFrame {
    pub(crate) sub_id: SubscriberId,
    pub(crate) run_id: u64,
    pub(crate) had_nested: bool,
}

pub(crate) struct ScopeStackGuard;
impl Drop for ScopeStackGuard {
    fn drop(&mut self) {
        let _ = crate::runtime::RUNTIME.try_with(|rt| {
            if let Ok(mut rt_mut) = rt.try_borrow_mut() {
                if rt_mut.scope_stack.len() > 1 {
                    rt_mut.scope_stack.pop();
                }
            }
        });
    }
}

pub(crate) struct UntrackedGuard {
    pub(crate) saved: Option<Vec<TrackingFrame>>,
    pub(crate) had_outer_tracking: bool,
}
impl Drop for UntrackedGuard {
    fn drop(&mut self) {
        if let Some(saved) = self.saved.take() {
            let had_outer = self.had_outer_tracking;
            let _ = crate::runtime::RUNTIME.try_with(|rt| {
                if let Ok(mut rt_mut) = rt.try_borrow_mut() {
                    if had_outer {
                        rt_mut.untracked_in_tracking_depth =
                            rt_mut.untracked_in_tracking_depth.saturating_sub(1);
                    }
                    rt_mut.tracking_stack = saved;
                }
            });
        }
    }
}

pub(crate) struct TrackingFrameGuard;
impl Drop for TrackingFrameGuard {
    fn drop(&mut self) {
        let should_flush = crate::runtime::RUNTIME
            .try_with(|rt| {
                if let Ok(mut rt_mut) = rt.try_borrow_mut() {
                    if let Some(frame) = rt_mut.tracking_stack.pop() {
                        if let Some(sub) = rt_mut.subscribers.get_mut(frame.sub_id.index as usize) {
                            if sub.alive
                                && sub.generation == frame.sub_id.generation
                                && sub.state == ComputedState::Computing
                            {
                                sub.state = ComputedState::Clean;
                            }
                        }
                    }
                    rt_mut.can_flush_immediately()
                        && !rt_mut.pending_subscribers.is_empty()
                        && !std::thread::panicking()
                } else {
                    false
                }
            })
            .unwrap_or(false);
        if should_flush {
            crate::runtime::flush_pending_subscribers();
        }
    }
}

pub(crate) struct BatchDepthGuard {
    pub(crate) flush_on_zero: bool,
}
impl Drop for BatchDepthGuard {
    fn drop(&mut self) {
        let should_flush = crate::runtime::RUNTIME
            .try_with(|rt| {
                if let Ok(mut rt_mut) = rt.try_borrow_mut() {
                    rt_mut.batch_depth = rt_mut.batch_depth.saturating_sub(1);
                    self.flush_on_zero
                        && rt_mut.can_flush_immediately()
                        && !std::thread::panicking()
                } else {
                    false
                }
            })
            .unwrap_or(false);
        if should_flush {
            crate::runtime::flush_pending_subscribers();
        }
    }
}

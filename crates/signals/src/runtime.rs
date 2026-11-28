//! Thread-local reactive arena, generational IDs, and ownership `Scope`.

pub(crate) use crate::arena_types::ComputedState;
pub use crate::arena_types::{ComputedId, EffectId, RuntimeStats, ScopeId, SignalId, SubscriberId};
use crate::arena_types::{ScopeSlot, SignalSlot, SubscriberSlot, TrackingFrame};
pub(crate) use crate::propagation::{ensure_signal_up_to_date, flush_pending_subscribers};
pub use crate::scope::{batch, current_scope_id, is_scope_alive, untracked, Scope};

use std::collections::BTreeSet;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

pub(crate) type ScopeCleanupEntry = (ScopeId, Option<Arc<AtomicBool>>, Rc<dyn Fn()>);

pub(crate) struct ReactiveRuntime {
    pub(crate) runtime_id: crate::session_hub::RuntimeId,
    pub(crate) runtime_tag_bits: u32,
    pub(crate) generation_base: u32,
    pub(crate) max_local_gen: u32,
    pub(crate) signals: Vec<SignalSlot>,
    pub(crate) free_signals: Vec<u32>,
    pub(crate) subscribers: Vec<SubscriberSlot>,
    pub(crate) free_subscribers: Vec<u32>,
    pub(crate) scopes: Vec<ScopeSlot>,
    pub(crate) free_scopes: Vec<u32>,
    pub(crate) scope_stack: Vec<ScopeId>,
    pub(crate) tracking_stack: Vec<TrackingFrame>,
    pub(crate) untracked_in_tracking_depth: usize,
    pub(crate) next_run_id: u64,
    pub(crate) batch_depth: usize,
    pub(crate) pending_subscribers: BTreeSet<(u32, SubscriberId)>,
    pub(crate) effect_executions: u64,
    pub(crate) deferred_drops: Vec<Rc<dyn Fn()>>,
    pub(crate) deferred_cleanups: Vec<Rc<dyn Fn()>>,
    pub(crate) scope_cleanups: Vec<ScopeCleanupEntry>,
    pub(crate) updating_signals: Vec<SignalId>,
}

impl Drop for ReactiveRuntime {
    fn drop(&mut self) {
        for (_, _, cb) in std::mem::take(&mut self.scope_cleanups) {
            cb();
        }
        for cb in std::mem::take(&mut self.deferred_cleanups) {
            cb();
        }
    }
}

impl ReactiveRuntime {
    pub(crate) fn new_with_tag(
        runtime_id: crate::session_hub::RuntimeId,
        runtime_tag_bits: u32,
        generation_base: u32,
    ) -> Self {
        let initial_local = (generation_base & crate::session_hub::LOCAL_GEN_MASK).max(1);
        let root_scope = ScopeSlot::new(0, None);
        Self {
            runtime_id,
            runtime_tag_bits,
            generation_base: runtime_tag_bits | initial_local,
            max_local_gen: initial_local,
            signals: Vec::with_capacity(256),
            free_signals: Vec::new(),
            subscribers: Vec::with_capacity(256),
            free_subscribers: Vec::new(),
            scopes: vec![root_scope],
            free_scopes: Vec::new(),
            scope_stack: vec![ScopeId::ROOT],
            tracking_stack: Vec::with_capacity(16),
            untracked_in_tracking_depth: 0,
            next_run_id: 1,
            batch_depth: 0,
            pending_subscribers: BTreeSet::new(),
            effect_executions: 0,
            deferred_drops: Vec::new(),
            deferred_cleanups: Vec::new(),
            scope_cleanups: Vec::new(),
            updating_signals: Vec::new(),
        }
    }

    fn record_bumped_gen(&mut self, next_gen: u32) {
        let local = next_gen & crate::session_hub::LOCAL_GEN_MASK;
        if local > self.max_local_gen {
            self.max_local_gen = local;
        }
    }

    pub(crate) fn reset_in_place(&mut self) {
        for (_, _, cb) in std::mem::take(&mut self.scope_cleanups) {
            cb();
        }
        for cb in std::mem::take(&mut self.deferred_cleanups) {
            cb();
        }
        let next_local = self
            .max_local_gen
            .saturating_add(1)
            .clamp(1, crate::session_hub::LOCAL_GEN_MASK);
        let next_gen = self.runtime_tag_bits | next_local;
        *self = Self::new_with_tag(self.runtime_id, self.runtime_tag_bits, next_gen);
    }

    pub(crate) fn active_scope_id(&self) -> ScopeId {
        self.scope_stack
            .iter()
            .rev()
            .copied()
            .find(|&id| self.is_scope_alive(id))
            .unwrap_or(ScopeId::ROOT)
    }

    pub(crate) const fn can_flush_immediately(&self) -> bool {
        self.batch_depth == 0
            && self.tracking_stack.is_empty()
            && self.untracked_in_tracking_depth == 0
    }

    pub(crate) fn is_scope_alive(&self, id: ScopeId) -> bool {
        self.scopes
            .get(id.index as usize)
            .is_some_and(|s| s.alive && s.generation == id.generation)
    }

    pub(crate) fn is_signal_alive(&self, id: SignalId) -> bool {
        self.signals
            .get(id.index as usize)
            .is_some_and(|s| s.alive && s.generation == id.generation)
    }

    pub(crate) fn is_subscriber_alive(&self, id: SubscriberId) -> bool {
        self.subscribers
            .get(id.index as usize)
            .is_some_and(|s| s.alive && s.generation == id.generation)
    }

    pub(crate) fn clear_subscriber_dependencies(&mut self, sub_id: SubscriberId) {
        let Some(sub_slot) = self.subscribers.get(sub_id.index as usize) else {
            return;
        };
        if sub_slot.generation != sub_id.generation {
            return;
        }
        self.clear_subscriber_dependencies_unchecked(sub_id.index as usize, sub_id);
    }

    pub(crate) fn dispose_subscriber(&mut self, sub_id: SubscriberId) {
        if !self.is_subscriber_alive(sub_id) {
            return;
        }
        let (owner_scope, run_scope, old_cb, next_gen, saturated) = {
            let sub_slot = &mut self.subscribers[sub_id.index as usize];
            sub_slot.alive = false;
            sub_slot.state = ComputedState::Clean;
            sub_slot.produced_signal = None;
            let cb = sub_slot.callback.take();
            let (next_gen, saturated) =
                crate::session_hub::bump_preserving_tag(sub_slot.generation);
            sub_slot.generation = next_gen;
            (
                sub_slot.owner_scope,
                sub_slot.run_scope.take(),
                cb,
                next_gen,
                saturated,
            )
        };
        self.record_bumped_gen(next_gen);
        if let Some(cb) = old_cb {
            self.deferred_drops.push(cb);
        }
        self.clear_subscriber_dependencies_unchecked(sub_id.index as usize, sub_id);
        self.pending_subscribers.retain(|&(_, s)| s != sub_id);
        if !saturated {
            self.free_subscribers.push(sub_id.index);
        }

        if owner_scope != ScopeId::ROOT {
            if let Some(scope_slot) = self
                .scopes
                .get_mut(owner_scope.index as usize)
                .filter(|s| s.alive && s.generation == owner_scope.generation)
            {
                scope_slot.owned_subscribers.retain(|&s| s != sub_id);
            }
        }
        if let Some(rscope) = run_scope {
            self.dispose_scope_recursive(rscope);
        }
    }

    fn clear_subscriber_dependencies_unchecked(&mut self, sub_idx: usize, sub_id: SubscriberId) {
        let Some(sub_slot) = self.subscribers.get_mut(sub_idx) else {
            return;
        };
        let deps = std::mem::take(&mut sub_slot.dependencies);
        for sig_id in deps {
            if let Some(sig_slot) = self.signals.get_mut(sig_id.index as usize) {
                if sig_slot.generation == sig_id.generation {
                    sig_slot.subscribers.retain(|&s| s != sub_id);
                    if sig_slot.last_subscribed_run.map(|(s, _)| s) == Some(sub_id) {
                        sig_slot.last_subscribed_run = None;
                    }
                }
            }
        }
    }

    pub(crate) fn allocate_child_scope(&mut self, parent: Option<ScopeId>) -> ScopeId {
        let effective_parent = parent.filter(|&p| p != ScopeId::ROOT && self.is_scope_alive(p));
        let (index, generation) = if let Some(idx) = self.free_scopes.pop() {
            let slot = &mut self.scopes[idx as usize];
            slot.alive = true;
            slot.parent = effective_parent;
            slot.owned_signals.clear();
            slot.owned_subscribers.clear();
            slot.owned_children.clear();
            (idx, slot.generation)
        } else {
            let idx = self.scopes.len() as u32;
            let gen = self.generation_base;
            self.scopes.push(ScopeSlot::new(gen, effective_parent));
            (idx, gen)
        };
        let new_id = ScopeId { index, generation };
        if let Some(pid) = effective_parent {
            if let Some(p) = self.scopes.get_mut(pid.index as usize) {
                p.owned_children.push(new_id);
            }
        }
        new_id
    }

    pub(crate) fn dispose_scope_recursive(&mut self, scope_id: ScopeId) {
        if scope_id == ScopeId::ROOT {
            return;
        }
        let Some(scope_slot) = self.scopes.get_mut(scope_id.index as usize) else {
            return;
        };
        if !scope_slot.alive || scope_slot.generation != scope_id.generation {
            return;
        }
        scope_slot.alive = false;
        let (next_scope_gen, scope_saturated) =
            crate::session_hub::bump_preserving_tag(scope_slot.generation);
        scope_slot.generation = next_scope_gen;
        let parent_id = scope_slot.parent.take();
        let children = std::mem::take(&mut scope_slot.owned_children);
        let subs = std::mem::take(&mut scope_slot.owned_subscribers);
        let sigs = std::mem::take(&mut scope_slot.owned_signals);
        self.record_bumped_gen(next_scope_gen);
        if !scope_saturated {
            self.free_scopes.push(scope_id.index);
        }

        if let Some(pid) = parent_id {
            if let Some(parent_slot) = self
                .scopes
                .get_mut(pid.index as usize)
                .filter(|p| p.alive && p.generation == pid.generation)
            {
                parent_slot.owned_children.retain(|&c| c != scope_id);
            }
        }

        for child_id in children {
            self.dispose_scope_recursive(child_id);
        }
        for sub_id in subs {
            self.dispose_subscriber(sub_id);
        }
        self.dispose_owned_signals_and_cleanups(scope_id, sigs);
    }

    fn dispose_owned_signals_and_cleanups(&mut self, scope_id: ScopeId, sigs: Vec<SignalId>) {
        let mut bumped_sigs = Vec::new();
        for sig_id in sigs {
            if let Some(slot) = self
                .signals
                .get_mut(sig_id.index as usize)
                .filter(|s| s.alive && s.generation == sig_id.generation)
            {
                let (next_sig_gen, sig_saturated) = slot.dispose();
                bumped_sigs.push(next_sig_gen);
                if !sig_saturated {
                    self.free_signals.push(sig_id.index);
                }
            }
        }
        for next_sig_gen in bumped_sigs {
            self.record_bumped_gen(next_sig_gen);
        }
        let mut remaining_cleanups = Vec::with_capacity(self.scope_cleanups.len());
        for (sid, active_opt, cb) in std::mem::take(&mut self.scope_cleanups) {
            if sid == scope_id {
                self.deferred_cleanups.push(cb);
            } else if active_opt
                .as_ref()
                .is_none_or(|a| a.load(std::sync::atomic::Ordering::Acquire))
            {
                remaining_cleanups.push((sid, active_opt, cb));
            }
        }
        self.scope_cleanups = remaining_cleanups;
    }
}

#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn register_scope_cleanup(
    scope_id: ScopeId,
    active_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    cb: impl Fn() + 'static,
) {
    RUNTIME.with(|rt| {
        let mut rt_mut = rt.borrow_mut();
        if rt_mut.scope_cleanups.len() >= 32 {
            rt_mut.scope_cleanups.retain(|(_, act, _)| {
                act.as_ref()
                    .is_none_or(|a| a.load(std::sync::atomic::Ordering::Acquire))
            });
        }
        if rt_mut.is_scope_alive(scope_id) {
            rt_mut
                .scope_cleanups
                .push((scope_id, active_flag, Rc::new(cb)));
        } else {
            cb();
        }
    });
}

pub(crate) use crate::session_hub::RUNTIME;

/// Resets the currently active session's reactive runtime (primarily for isolated tests and benchmarks).
pub fn reset_reactive_runtime() {
    crate::session_hub::active_session().reset();
}

/// Returns the total number of subscriber executions performed on this thread.
pub fn total_subscriber_executions() -> u64 {
    RUNTIME.with(|rt| rt.borrow().effect_executions)
}

/// Returns current diagnostic statistics (`RuntimeStats`) for the thread-local reactive arena.
pub fn debug_stats() -> RuntimeStats {
    let live_async_pollers = crate::frame_clock::live_async_poller_count();
    RUNTIME.with(|rt| {
        let rt = rt.borrow();
        let live_subscribers = rt.subscribers.iter().filter(|s| s.alive).count();
        let live_effects = rt
            .subscribers
            .iter()
            .filter(|s| s.alive && s.is_effect)
            .count();
        let live_computed = live_subscribers.saturating_sub(live_effects);
        let live_scopes = rt
            .scopes
            .iter()
            .enumerate()
            .filter(|&(idx, s)| idx > 0 && s.alive)
            .count();
        RuntimeStats {
            live_signals: rt.signals.iter().filter(|s| s.alive).count(),
            live_computed,
            live_subscribers,
            live_effects,
            live_scopes,
            total_subscriber_entries: rt
                .signals
                .iter()
                .filter(|s| s.alive)
                .map(|s| s.subscribers.len())
                .sum(),
            live_async_pollers,
            retained_disposed_fallbacks: 0,
        }
    })
}

pub(crate) fn drain_deferred_drops_only() {
    loop {
        let (drops, cleanups) = RUNTIME
            .try_with(|rt| {
                rt.try_borrow_mut()
                    .ok()
                    .map(|mut g| {
                        (
                            std::mem::take(&mut g.deferred_drops),
                            std::mem::take(&mut g.deferred_cleanups),
                        )
                    })
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        if drops.is_empty() && cleanups.is_empty() {
            break;
        }
        for cb in cleanups {
            cb();
        }
        drop(drops);
    }
}

pub(crate) fn drain_deferred_drops() {
    drain_deferred_drops_only();
    crate::session_hub::flush_deferred_scope_disposals();
}

//! Glitch-free push-pull propagation, topological ordering, and dynamic dependency tracking.

use crate::arena_types::{
    BatchDepthGuard, ComputedState, ScopeId, SignalId, SubscriberId, SubscriberSlot, TrackingFrame,
    TrackingFrameGuard,
};
use crate::runtime::{drain_deferred_drops, ReactiveRuntime, Scope, RUNTIME};
use std::rc::Rc;

pub(crate) const EFFECT_TOPO_DEPTH: u32 = 1_000_000;

impl ReactiveRuntime {
    /// Marks direct `Computed` subscribers `Dirty`, transitive downstream `Computed`s `Check`,
    /// and direct `Effect` subscribers pending at `EFFECT_TOPO_DEPTH`.
    pub(crate) fn notify_signal_changed(&mut self, sig_id: SignalId) {
        let Some(sig_slot) = self.signals.get(sig_id.index as usize) else {
            return;
        };
        if !sig_slot.alive || sig_slot.generation != sig_id.generation {
            return;
        }
        let direct_subs = sig_slot.subscribers.clone();
        let mut check_queue: Vec<SignalId> = Vec::new();

        for sub_id in direct_subs {
            if !self.is_subscriber_alive(sub_id) {
                continue;
            }
            let sub = &mut self.subscribers[sub_id.index as usize];
            if sub.is_effect {
                self.pending_subscribers.insert((EFFECT_TOPO_DEPTH, sub_id));
            } else {
                sub.state = ComputedState::Dirty;
                let depth = sub.depth;
                if let Some(prod_sig) = sub.produced_signal {
                    check_queue.push(prod_sig);
                }
                self.pending_subscribers.insert((depth, sub_id));
            }
        }

        while let Some(curr_sig_id) = check_queue.pop() {
            let Some(curr_sig) = self.signals.get(curr_sig_id.index as usize) else {
                continue;
            };
            if !curr_sig.alive || curr_sig.generation != curr_sig_id.generation {
                continue;
            }
            let down_subs = curr_sig.subscribers.clone();
            for down_sub_id in down_subs {
                if !self.is_subscriber_alive(down_sub_id) {
                    continue;
                }
                let down_sub = &mut self.subscribers[down_sub_id.index as usize];
                if !down_sub.is_effect && down_sub.state == ComputedState::Clean {
                    down_sub.state = ComputedState::Check;
                    let depth = down_sub.depth;
                    if let Some(next_sig) = down_sub.produced_signal {
                        check_queue.push(next_sig);
                    }
                    self.pending_subscribers.insert((depth, down_sub_id));
                }
            }
        }
    }
}

/// Pulls any dirty/check upstream `Computed` producer of `sig_id` before reading its value.
pub(crate) fn ensure_signal_up_to_date(sig_id: SignalId) {
    let maybe_producer = RUNTIME.with(|rt| {
        let rt_ref = rt.borrow();
        if !rt_ref.is_signal_alive(sig_id) {
            return None;
        }
        let prod = rt_ref.signals[sig_id.index as usize].producer?;
        if rt_ref.is_subscriber_alive(prod)
            && rt_ref.subscribers[prod.index as usize].state != ComputedState::Clean
        {
            Some(prod)
        } else {
            None
        }
    });

    if let Some(comp_id) = maybe_producer {
        ensure_computed_up_to_date(comp_id);
    }
}

/// Ensures `comp_id` is `ComputedState::Clean`, pulling upstream `Check` dependencies first
/// and only invoking `comp_id`'s recompute closure when its state is `ComputedState::Dirty`.
pub(crate) fn ensure_computed_up_to_date(comp_id: SubscriberId) {
    let (initial_state, upstream_producers) = RUNTIME.with(|rt| {
        let rt_ref = rt.borrow();
        if !rt_ref.is_subscriber_alive(comp_id) {
            return (ComputedState::Clean, Vec::new());
        }
        let slot = &rt_ref.subscribers[comp_id.index as usize];
        if slot.state != ComputedState::Check {
            return (slot.state, Vec::new());
        }
        let mut prods = Vec::new();
        for &dep_sig in &slot.dependencies {
            if rt_ref.is_signal_alive(dep_sig) {
                if let Some(p) = rt_ref.signals[dep_sig.index as usize].producer {
                    prods.push(p);
                }
            }
        }
        (ComputedState::Check, prods)
    });

    if initial_state == ComputedState::Clean {
        return;
    }

    if initial_state == ComputedState::Check {
        for up_id in upstream_producers {
            ensure_computed_up_to_date(up_id);
        }
    }

    let maybe_cb = RUNTIME.with(|rt| {
        let mut rt_mut = rt.borrow_mut();
        if !rt_mut.is_subscriber_alive(comp_id) {
            return None;
        }
        let slot = &mut rt_mut.subscribers[comp_id.index as usize];
        match slot.state {
            ComputedState::Clean => None,
            ComputedState::Check => {
                slot.state = ComputedState::Clean;
                None
            }
            ComputedState::Dirty => {
                slot.state = ComputedState::Clean;
                slot.callback.clone()
            }
        }
    });

    if let Some(cb) = maybe_cb {
        cb();
    }
}

pub(crate) fn flush_pending_subscribers() {
    RUNTIME.with(|rt| rt.borrow_mut().batch_depth += 1);
    let _guard = BatchDepthGuard {
        flush_on_zero: false,
    };
    loop {
        let next = RUNTIME.with(|rt| {
            let mut rt_mut = rt.borrow_mut();
            let sub_id = rt_mut.pending_subscribers.pop_first()?.1;
            if !rt_mut.is_subscriber_alive(sub_id) {
                return Some((sub_id, false, None));
            }
            let slot = &rt_mut.subscribers[sub_id.index as usize];
            Some((sub_id, slot.is_effect, slot.callback.clone()))
        });
        let Some((sub_id, is_effect, maybe_cb)) = next else {
            break;
        };
        if is_effect {
            if let Some(cb) = maybe_cb {
                run_effect_tracking(sub_id, || cb());
            }
        } else {
            ensure_computed_up_to_date(sub_id);
        }
    }
}

/// Runs a `Computed` closure with fresh dynamic dependency re-tracking and topological depth calculation.
pub(crate) fn run_computed_tracking<R>(sub_id: SubscriberId, cb: impl FnOnce() -> R) -> R {
    RUNTIME.with(|rt| {
        let mut rt_mut = rt.borrow_mut();
        rt_mut.clear_subscriber_dependencies(sub_id);
        if let Some(slot) = rt_mut
            .subscribers
            .get_mut(sub_id.index as usize)
            .filter(|s| s.alive && s.generation == sub_id.generation)
        {
            slot.state = ComputedState::Clean;
            slot.depth = 1;
        }
        let run_id = rt_mut.next_run_id;
        rt_mut.next_run_id = rt_mut.next_run_id.wrapping_add(1);
        if let Some(parent_frame) = rt_mut.tracking_stack.last_mut() {
            parent_frame.had_nested = true;
        }
        rt_mut.tracking_stack.push(TrackingFrame {
            sub_id,
            run_id,
            had_nested: false,
        });
        rt_mut.effect_executions += 1;
    });
    let _track_guard = TrackingFrameGuard;
    cb()
}

/// Runs an `Effect` closure inside a fresh per-run child `Scope` with dynamic dependency re-tracking.
pub(crate) fn run_effect_tracking<R>(sub_id: SubscriberId, cb: impl FnOnce() -> R) -> R {
    let maybe_run_scope = RUNTIME.with(|rt| {
        let mut rt_mut = rt.borrow_mut();
        rt_mut.clear_subscriber_dependencies(sub_id);
        let (owner_scope, prev_run_scope) = rt_mut
            .subscribers
            .get_mut(sub_id.index as usize)
            .filter(|s| s.alive && s.generation == sub_id.generation)
            .map(|s| (s.owner_scope, s.run_scope.take()))
            .unwrap_or((ScopeId::ROOT, None));

        if let Some(old_scope) = prev_run_scope {
            rt_mut.dispose_scope_recursive(old_scope);
        }

        let new_run_scope = if rt_mut.is_subscriber_alive(sub_id) {
            let child = rt_mut.allocate_child_scope(Some(owner_scope));
            rt_mut.subscribers[sub_id.index as usize].run_scope = Some(child);
            Some(child)
        } else {
            None
        };

        let run_id = rt_mut.next_run_id;
        rt_mut.next_run_id = rt_mut.next_run_id.wrapping_add(1);
        if let Some(parent_frame) = rt_mut.tracking_stack.last_mut() {
            parent_frame.had_nested = true;
        }
        rt_mut.tracking_stack.push(TrackingFrame {
            sub_id,
            run_id,
            had_nested: false,
        });
        rt_mut.effect_executions += 1;
        new_run_scope
    });
    drain_deferred_drops();
    let _track_guard = TrackingFrameGuard;
    if let Some(rscope) = maybe_run_scope {
        Scope::from_id(rscope).run(cb)
    } else {
        cb()
    }
}

pub(crate) fn allocate_subscriber_slot(depth: u32, is_effect: bool) -> SubscriberId {
    RUNTIME.with(|rt| {
        let mut rt = rt.borrow_mut();
        let scope_id = rt.active_scope_id();
        let (index, generation) = if let Some(idx) = rt.free_subscribers.pop() {
            let slot = &mut rt.subscribers[idx as usize];
            slot.alive = true;
            slot.is_effect = is_effect;
            slot.state = ComputedState::Clean;
            slot.depth = depth;
            slot.owner_scope = scope_id;
            slot.run_scope = None;
            slot.produced_signal = None;
            slot.dependencies.clear();
            slot.callback = None;
            (idx, slot.generation)
        } else {
            let idx = rt.subscribers.len() as u32;
            let gen = rt.generation_base;
            rt.subscribers.push(SubscriberSlot {
                generation: gen,
                alive: true,
                is_effect,
                state: ComputedState::Clean,
                depth,
                owner_scope: scope_id,
                run_scope: None,
                produced_signal: None,
                dependencies: Vec::new(),
                callback: None,
            });
            (idx, gen)
        };
        let id = SubscriberId { index, generation };
        if scope_id != ScopeId::ROOT {
            if let Some(s) = rt
                .scopes
                .get_mut(scope_id.index as usize)
                .filter(|s| s.alive && s.generation == scope_id.generation)
            {
                s.owned_subscribers.push(id);
            }
        }
        id
    })
}

pub(crate) fn set_subscriber_callback(sub_id: SubscriberId, callback: Rc<dyn Fn()>) {
    RUNTIME.with(|rt| {
        let mut rt = rt.borrow_mut();
        if rt.is_subscriber_alive(sub_id) {
            rt.subscribers[sub_id.index as usize].callback = Some(callback);
        }
    });
}

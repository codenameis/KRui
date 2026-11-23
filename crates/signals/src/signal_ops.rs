//! Low-level `SignalSlot` read guards, in-place `Rc` storage reuse, and dependency subscription recording.

use crate::arena_types::{empty_any_rc, SignalSlot, TrackingFrame};
use crate::frame_clock::FastPathTransform;
use crate::runtime::{
    flush_pending_subscribers, ReactiveRuntime, ScopeId, SignalId, SubscriberId, RUNTIME,
};
use std::any::{Any, TypeId};
use std::rc::Rc;

pub(crate) fn allocate_signal_slot(
    rt: &mut ReactiveRuntime,
    value_rc: Rc<dyn Any>,
    initial_rc: Rc<dyn Any>,
    producer: Option<SubscriberId>,
    is_frame_driven: bool,
) -> SignalId {
    let id = if let Some(idx) = rt.free_signals.pop() {
        let slot = &mut rt.signals[idx as usize];
        let generation = slot.generation;
        slot.alive = true;
        slot.value = value_rc;
        slot.initial_value = initial_rc;
        slot.producer = producer;
        slot.subscribers.clear();
        slot.last_subscribed_run = None;
        slot.is_frame_driven = is_frame_driven;
        slot.borrow_depth = 0;
        slot.deferred_notify = false;
        SignalId {
            index: idx,
            generation,
        }
    } else {
        let idx = rt.signals.len() as u32;
        let gen = rt.generation_base;
        rt.signals.push(SignalSlot {
            generation: gen,
            alive: true,
            value: value_rc,
            initial_value: initial_rc,
            producer,
            subscribers: Vec::new(),
            last_subscribed_run: None,
            is_frame_driven,
            borrow_depth: 0,
            deferred_notify: false,
        });
        SignalId {
            index: idx,
            generation: gen,
        }
    };
    if let Some(prod_id) = producer {
        if rt.is_subscriber_alive(prod_id) {
            rt.subscribers[prod_id.index as usize].produced_signal = Some(id);
        }
    }
    let scope_id = rt.active_scope_id();
    if scope_id != ScopeId::ROOT {
        if let Some(scope_slot) = rt.scopes.get_mut(scope_id.index as usize) {
            if scope_slot.alive && scope_slot.generation == scope_id.generation {
                scope_slot.owned_signals.push(id);
            }
        }
    }
    id
}

pub(crate) struct SignalReadGuard {
    pub(crate) id: SignalId,
}

impl Drop for SignalReadGuard {
    fn drop(&mut self) {
        let should_flush = RUNTIME
            .try_with(|rt| {
                let Ok(mut rt_mut) = rt.try_borrow_mut() else {
                    return false;
                };
                if !rt_mut.is_signal_alive(self.id) {
                    return false;
                }
                let should_notify = {
                    let slot = &mut rt_mut.signals[self.id.index as usize];
                    slot.borrow_depth = slot.borrow_depth.saturating_sub(1);
                    if slot.borrow_depth == 0 && slot.deferred_notify {
                        slot.deferred_notify = false;
                        true
                    } else {
                        false
                    }
                };
                if !should_notify {
                    return false;
                }
                rt_mut.notify_signal_changed(self.id);
                rt_mut.batch_depth == 0
                    && rt_mut.tracking_stack.is_empty()
                    && !std::thread::panicking()
            })
            .unwrap_or(false);

        if should_flush {
            flush_pending_subscribers();
        }
    }
}

pub(crate) fn default_disposed_fallback<T: 'static>() -> Option<Rc<dyn Any>> {
    let tid = TypeId::of::<T>();
    if tid == TypeId::of::<String>() {
        Some(Rc::new(String::new()))
    } else if tid == TypeId::of::<i32>() {
        Some(Rc::new(0_i32))
    } else if tid == TypeId::of::<i64>() {
        Some(Rc::new(0_i64))
    } else if tid == TypeId::of::<u32>() {
        Some(Rc::new(0_u32))
    } else if tid == TypeId::of::<u64>() {
        Some(Rc::new(0_u64))
    } else if tid == TypeId::of::<usize>() {
        Some(Rc::new(0_usize))
    } else if tid == TypeId::of::<isize>() {
        Some(Rc::new(0_isize))
    } else if tid == TypeId::of::<f32>() {
        Some(Rc::new(0.0_f32))
    } else if tid == TypeId::of::<f64>() {
        Some(Rc::new(0.0_f64))
    } else if tid == TypeId::of::<bool>() {
        Some(Rc::new(false))
    } else if tid == TypeId::of::<FastPathTransform>() {
        Some(Rc::new(FastPathTransform::IDENTITY))
    } else {
        default_disposed_extended(tid)
    }
}

fn default_disposed_extended(tid: TypeId) -> Option<Rc<dyn Any>> {
    if tid == TypeId::of::<()>() {
        Some(Rc::new(()))
    } else if tid == TypeId::of::<char>() {
        Some(Rc::new('\0'))
    } else if tid == TypeId::of::<i8>() {
        Some(Rc::new(0_i8))
    } else if tid == TypeId::of::<u8>() {
        Some(Rc::new(0_u8))
    } else if tid == TypeId::of::<i16>() {
        Some(Rc::new(0_i16))
    } else if tid == TypeId::of::<u16>() {
        Some(Rc::new(0_u16))
    } else if tid == TypeId::of::<Vec<String>>() {
        Some(Rc::new(Vec::<String>::new()))
    } else if tid == TypeId::of::<Vec<i32>>() {
        Some(Rc::new(Vec::<i32>::new()))
    } else if tid == TypeId::of::<Vec<u8>>() {
        Some(Rc::new(Vec::<u8>::new()))
    } else if tid == TypeId::of::<Option<String>>() {
        Some(Rc::new(None::<String>))
    } else if tid == TypeId::of::<Option<i32>>() {
        Some(Rc::new(None::<i32>))
    } else if tid == TypeId::of::<Option<usize>>() {
        Some(Rc::new(None::<usize>))
    } else if tid == TypeId::of::<Option<f32>>() {
        Some(Rc::new(None::<f32>))
    } else if tid == TypeId::of::<Option<bool>>() {
        Some(Rc::new(None::<bool>))
    } else {
        None
    }
}

pub(crate) fn store_value_in_slot<T: 'static>(slot: &mut SignalSlot, new_value: T) {
    let prev = std::mem::replace(&mut slot.value, empty_any_rc());
    match Rc::downcast::<T>(prev) {
        Ok(mut typed_rc) => {
            if let Some(mut_ref) = Rc::get_mut(&mut typed_rc) {
                *mut_ref = new_value;
                slot.value = typed_rc;
            } else {
                slot.value = Rc::new(new_value);
            }
        }
        Err(_) => {
            slot.value = Rc::new(new_value);
        }
    }
}

pub(crate) fn record_active_subscription(rt: &mut ReactiveRuntime, sig_id: SignalId) {
    let Some(&TrackingFrame {
        sub_id,
        run_id,
        had_nested,
    }) = rt.tracking_stack.last()
    else {
        return;
    };
    if !rt.is_subscriber_alive(sub_id) {
        return;
    }

    let (already_subscribed, upstream_prod) = {
        let slot = &rt.signals[sig_id.index as usize];
        let dup = if slot.last_subscribed_run == Some((sub_id, run_id)) {
            true
        } else if had_nested {
            slot.subscribers.contains(&sub_id)
        } else {
            false
        };
        (dup, slot.producer)
    };

    let upstream_depth = upstream_prod
        .filter(|&p| rt.is_subscriber_alive(p))
        .map(|p| rt.subscribers[p.index as usize].depth)
        .unwrap_or(0);

    if !already_subscribed {
        let slot = &mut rt.signals[sig_id.index as usize];
        slot.subscribers.push(sub_id);
        slot.last_subscribed_run = Some((sub_id, run_id));
        let sub_slot = &mut rt.subscribers[sub_id.index as usize];
        sub_slot.dependencies.push(sig_id);
    }

    let sub_slot = &mut rt.subscribers[sub_id.index as usize];
    if !sub_slot.is_effect {
        sub_slot.depth = sub_slot.depth.max(upstream_depth.saturating_add(1));
    }
}

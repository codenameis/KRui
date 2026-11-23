//! Ownership `Scope` handle, `untracked`, and `batch` combinators for `krui-signals`.

use crate::arena_types::{BatchDepthGuard, ScopeId, ScopeStackGuard, UntrackedGuard};
use crate::runtime::{drain_deferred_drops_only, RUNTIME};
use std::marker::PhantomData;

/// Returns the currently active `ScopeId`.
pub fn current_scope_id() -> ScopeId {
    RUNTIME.with(|rt| rt.borrow().active_scope_id())
}

/// Returns `true` if `scope_id` has not been disposed.
pub fn is_scope_alive(scope_id: ScopeId) -> bool {
    if scope_id == ScopeId::ROOT {
        return true;
    }
    let Some(sess) = crate::session_hub::session_for_generation(scope_id.generation) else {
        return false;
    };
    sess.inner
        .runtime
        .try_borrow()
        .is_ok_and(|rt| rt.is_scope_alive(scope_id))
}

/// Ownership scope for reactive `Signal`s, `Computed`s, `Effect`s, and `FrameClock` drivers.
///
/// Strictly `!Send` and `!Sync` so scopes remain confined to their owning UI thread.
///
/// ```compile_fail
/// use krui_signals::Scope;
/// fn assert_send<T: Send>() {}
/// assert_send::<Scope>();
/// ```
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Scope {
    id: ScopeId,
    _marker: PhantomData<*const ()>,
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}

impl Scope {
    pub(crate) const fn from_id(id: ScopeId) -> Self {
        Self {
            id,
            _marker: PhantomData,
        }
    }

    /// Allocates a new reactive `Scope` (registering as a child of the currently active scope, if any).
    pub fn new() -> Self {
        let id = RUNTIME.with(|rt| {
            let mut rt = rt.borrow_mut();
            let parent = rt.scope_stack.last().copied();
            rt.allocate_child_scope(parent)
        });
        Self::from_id(id)
    }

    /// Allocates a new `Scope` and immediately runs `f` inside it.
    pub fn with(f: impl FnOnce()) -> Self {
        let scope = Self::new();
        scope.run(f);
        scope
    }

    /// Returns the currently active `Scope`.
    pub fn current() -> Self {
        Self::from_id(current_scope_id())
    }

    /// Returns the permanent root `Scope`.
    pub const fn root() -> Self {
        Self::from_id(ScopeId::ROOT)
    }

    /// Returns the underlying `ScopeId`.
    pub const fn id(self) -> ScopeId {
        self.id
    }

    /// Executes `f` with this `Scope` pushed as the active ownership scope.
    pub fn run<R>(self, f: impl FnOnce() -> R) -> R {
        RUNTIME.with(|rt| rt.borrow_mut().scope_stack.push(self.id));
        let _guard = ScopeStackGuard;
        f()
    }

    /// Disposes all child scopes, `Effect`s, `Signal`s/`Computed`s, and `FrameClock` drivers
    /// owned by this `Scope`, returning their arena slots to the free lists.
    pub fn dispose(self) {
        if self.id == ScopeId::ROOT {
            return;
        }
        let Some(sess) = crate::session_hub::session_for_generation(self.id.generation) else {
            return;
        };
        sess.enter(|| {
            let mut disposed_now = false;
            if let Ok(mut rt_mut) = sess.inner.runtime.try_borrow_mut() {
                rt_mut.dispose_scope_recursive(self.id);
                disposed_now = true;
            } else {
                sess.inner.pending_disposals.borrow_mut().push(self.id);
            }
            if disposed_now {
                drain_deferred_drops_only();
                crate::session_hub::flush_session_disposals(&sess);
            }
        });
    }
}

/// Executes `f` without subscribing any enclosing `Effect` or `Computed` to signals read directly in `f`
/// (while allowing newly created child `Effect`s/`Computed`s inside `f` to track their own dependencies).
pub fn untracked<R>(f: impl FnOnce() -> R) -> R {
    let saved = RUNTIME.with(|rt| std::mem::take(&mut rt.borrow_mut().tracking_stack));
    let _guard = UntrackedGuard { saved: Some(saved) };
    f()
}

/// Coalesces all signal writes inside `f` so downstream `Computed` values and `Effect`s
/// execute only once after `f` finishes, preventing intermediate glitches.
pub fn batch<R>(f: impl FnOnce() -> R) -> R {
    struct FlushPendingDisposalsOnDrop;
    impl Drop for FlushPendingDisposalsOnDrop {
        fn drop(&mut self) {
            crate::session_hub::flush_deferred_scope_disposals();
        }
    }
    let _flush_disposals = FlushPendingDisposalsOnDrop;
    RUNTIME.with(|rt| rt.borrow_mut().batch_depth += 1);
    let _guard = BatchDepthGuard {
        flush_on_zero: true,
    };
    f()
}

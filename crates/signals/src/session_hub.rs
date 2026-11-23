//! Per-session reactive runtime isolation, wakeup queues, and scoped async pollers.

use crate::arena_types::ScopeId;
use crate::frame_clock::ScopedAsyncPoller;
use crate::runtime::ReactiveRuntime;
use crate::waker::UiWaker;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::{Rc, Weak};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, OnceLock, RwLock};

pub(crate) const SESSION_TAG_SHIFT: u32 = 20;
const MAX_SESSION_TAGS: usize = 4095;
pub(crate) const LOCAL_GEN_MASK: u32 = (1 << SESSION_TAG_SHIFT) - 1;

static NEXT_RUNTIME_ID: AtomicU32 = AtomicU32::new(1);
static GLOBAL_FALLBACK_WAKER: OnceLock<RwLock<Option<Arc<dyn UiWaker>>>> = OnceLock::new();

/// Unique monotonic identifier isolating a window or host's reactive session.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct RuntimeId(pub u32);

/// Increments the 20-bit local generation while preserving the upper 12-bit session tag.
///
/// Returns `(next_generation, saturated)`. When `saturated` is `true`, `next_generation` has
/// local bits `0` (never matching any live handle) and the caller must retire the arena slot.
pub(crate) fn bump_preserving_tag(gen: u32) -> (u32, bool) {
    let tag = gen & !LOCAL_GEN_MASK;
    let local = gen & LOCAL_GEN_MASK;
    if local >= LOCAL_GEN_MASK {
        (tag, true)
    } else {
        (tag | (local + 1), false)
    }
}

pub(crate) fn set_global_fallback_waker(next: Option<Arc<dyn UiWaker>>) {
    let lock = GLOBAL_FALLBACK_WAKER.get_or_init(|| RwLock::new(None));
    if let Ok(mut guard) = lock.write() {
        *guard = next;
    }
}

pub(crate) fn wake_global_fallback_waker() {
    if let Some(lock) = GLOBAL_FALLBACK_WAKER.get() {
        if let Ok(guard) = lock.read() {
            if let Some(waker) = guard.as_ref() {
                waker.wake();
            }
        }
    }
}

pub(crate) type UiTask = Box<dyn FnOnce() + Send>;

pub(crate) struct SessionWakerState {
    pub(crate) runtime_id: RuntimeId,
    pub(crate) waker: RwLock<Option<Arc<dyn UiWaker>>>,
    pub(crate) ui_tasks: Mutex<VecDeque<UiTask>>,
}

impl SessionWakerState {
    fn new(runtime_id: RuntimeId) -> Self {
        Self {
            runtime_id,
            waker: RwLock::new(None),
            ui_tasks: Mutex::new(VecDeque::new()),
        }
    }

    pub(crate) fn try_wake(&self) -> bool {
        if let Ok(guard) = self.waker.read() {
            if let Some(waker) = guard.as_ref() {
                waker.wake();
                return true;
            }
        }
        false
    }

    pub(crate) fn wake(&self) {
        let _ = self.try_wake();
    }

    pub(crate) fn post_task(&self, task: UiTask) {
        if let Ok(mut q) = self.ui_tasks.lock() {
            q.push_back(task);
        }
        self.wake();
    }

    pub(crate) fn set_waker(&self, next: Option<Arc<dyn UiWaker>>) {
        if let Ok(mut guard) = self.waker.write() {
            *guard = next;
        }
    }

    fn has_tasks(&self) -> bool {
        self.ui_tasks.lock().is_ok_and(|q| !q.is_empty())
    }

    fn pop_task(&self) -> Option<UiTask> {
        self.ui_tasks.lock().ok()?.pop_front()
    }

    fn clear_tasks(&self) {
        if let Ok(mut q) = self.ui_tasks.lock() {
            q.clear();
        }
    }
}

/// Thread-safe (`Send + Sync`) wakeup & UI-thread continuation handle bound to one `ReactiveSession`.
#[derive(Clone)]
pub struct SessionWakeHandle {
    state: Arc<SessionWakerState>,
}

impl SessionWakeHandle {
    /// Returns the `RuntimeId` of the session this handle wakes.
    pub fn runtime_id(&self) -> RuntimeId {
        self.state.runtime_id
    }

    /// Wakes only the UI loop associated with this session.
    pub fn wake(&self) {
        self.state.wake();
    }

    /// Enqueues a closure to run on this session's UI thread and wakes its UI loop.
    pub fn post_to_ui(&self, task: impl FnOnce() + Send + 'static) {
        self.state.post_task(Box::new(task));
    }
}

struct ThreadTagSlot {
    weak: Weak<ReactiveSessionInner>,
    min_local_gen: u32,
}

#[derive(Default)]
struct ThreadSessionTable {
    slots: Vec<ThreadTagSlot>,
}

impl ThreadSessionTable {
    fn reserve_tag(&mut self) -> (usize, u32, u32) {
        for (idx, slot) in self.slots.iter().enumerate() {
            if slot.weak.strong_count() == 0 && slot.min_local_gen < LOCAL_GEN_MASK {
                let tag = (idx + 1) as u32;
                let tag_bits = tag << SESSION_TAG_SHIFT;
                return (idx, tag_bits, tag_bits | slot.min_local_gen.max(1));
            }
        }
        if self.slots.len() < MAX_SESSION_TAGS {
            let idx = self.slots.len();
            self.slots.push(ThreadTagSlot {
                weak: Weak::new(),
                min_local_gen: 1,
            });
            let tag = (idx + 1) as u32;
            let tag_bits = tag << SESSION_TAG_SHIFT;
            return (idx, tag_bits, tag_bits | 1);
        }
        panic!("Exceeded maximum of {MAX_SESSION_TAGS} concurrent ReactiveSessions on one thread");
    }
}

pub(crate) struct ReactiveSessionInner {
    pub(crate) id: RuntimeId,
    pub(crate) tag_slot_index: usize,
    pub(crate) runtime: RefCell<ReactiveRuntime>,
    pub(crate) async_pollers: RefCell<Vec<ScopedAsyncPoller>>,
    pub(crate) pending_disposals: RefCell<Vec<ScopeId>>,
    pub(crate) waker_state: Arc<SessionWakerState>,
}

impl Drop for ReactiveSessionInner {
    fn drop(&mut self) {
        self.waker_state.clear_tasks();
        let max_local = self
            .runtime
            .try_borrow()
            .map(|rt| rt.max_local_gen)
            .unwrap_or(1);
        let slot_idx = self.tag_slot_index;
        let _ = THREAD_SESSIONS.try_with(|table| {
            if let Ok(mut tbl) = table.try_borrow_mut() {
                if let Some(slot) = tbl.slots.get_mut(slot_idx) {
                    slot.min_local_gen = max_local.saturating_add(1).min(LOCAL_GEN_MASK);
                }
            }
        });
    }
}

/// Isolated reactive arena, async poller list, and UI wakeup channel for a single window or host.
///
/// Multiple windows on the same UI thread run independent `ReactiveSession` instances via [`Self::enter`].
#[derive(Clone)]
pub struct ReactiveSession {
    pub(crate) inner: Rc<ReactiveSessionInner>,
}

impl ReactiveSession {
    /// Returns the currently active thread-local [`ReactiveSession`].
    pub fn current() -> Self {
        active_session()
    }

    /// Creates a brand-new isolated reactive session with its own `RuntimeId`, arena, and `UiWaker`.
    pub fn new() -> Self {
        let id = RuntimeId(NEXT_RUNTIME_ID.fetch_add(1, Ordering::Relaxed));
        let (slot_idx, tag_bits, initial_gen) =
            THREAD_SESSIONS.with(|table| table.borrow_mut().reserve_tag());
        let inner = Rc::new(ReactiveSessionInner {
            id,
            tag_slot_index: slot_idx,
            runtime: RefCell::new(ReactiveRuntime::new_with_tag(id, tag_bits, initial_gen)),
            async_pollers: RefCell::new(Vec::new()),
            pending_disposals: RefCell::new(Vec::new()),
            waker_state: Arc::new(SessionWakerState::new(id)),
        });
        THREAD_SESSIONS.with(|table| {
            table.borrow_mut().slots[slot_idx].weak = Rc::downgrade(&inner);
        });
        Self { inner }
    }

    /// Returns this session's unique [`RuntimeId`].
    pub fn id(&self) -> RuntimeId {
        self.inner.id
    }

    /// Returns a `Send + Sync` wake handle targeting only this session.
    pub fn wake_handle(&self) -> SessionWakeHandle {
        SessionWakeHandle {
            state: self.inner.waker_state.clone(),
        }
    }

    /// Installs or clears the OS `UiWaker` for this session.
    pub fn set_ui_waker(&self, waker: Option<Arc<dyn UiWaker>>) {
        self.inner.waker_state.set_waker(waker);
    }

    pub(crate) fn try_wake(&self) -> bool {
        self.inner.waker_state.try_wake()
    }

    /// Wakes this session's UI loop immediately.
    pub fn wake(&self) {
        self.inner.waker_state.wake();
    }

    /// Activates this session on the current thread for the duration of `f`.
    pub fn enter<R>(&self, f: impl FnOnce() -> R) -> R {
        SESSION_STACK.with(|stack| stack.borrow_mut().push(self.clone()));
        struct ExitGuard;
        impl Drop for ExitGuard {
            fn drop(&mut self) {
                let _ = SESSION_STACK.try_with(|stack| {
                    let _ = stack.borrow_mut().pop();
                });
            }
        }
        let _guard = ExitGuard;
        f()
    }

    /// Drains pending UI tasks and polls all registered async pollers belonging to this session.
    pub fn poll_async_tasks(&self) -> bool {
        self.enter(crate::frame_clock::poll_active_session_tasks)
    }

    /// Clears this session's queued UI tasks, pollers, pending disposals, and reactive runtime state.
    pub fn reset(&self) {
        self.inner.pending_disposals.borrow_mut().clear();
        self.inner.async_pollers.borrow_mut().clear();
        self.inner.waker_state.clear_tasks();
        let mut rt = self.inner.runtime.borrow_mut();
        rt.reset_in_place();
    }

    pub(crate) fn drain_ui_tasks(&self) -> bool {
        if !self.inner.waker_state.has_tasks() {
            return false;
        }
        let mut ran = false;
        crate::batch(|| {
            while let Some(task) = self.inner.waker_state.pop_task() {
                ran = true;
                task();
            }
        });
        ran
    }
}

impl Default for ReactiveSession {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    static THREAD_SESSIONS: RefCell<ThreadSessionTable> = RefCell::new(ThreadSessionTable::default());
    static DEFAULT_SESSION: ReactiveSession = ReactiveSession::new();
    static SESSION_STACK: RefCell<Vec<ReactiveSession>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn has_entered_session() -> bool {
    SESSION_STACK
        .try_with(|stack| !stack.borrow().is_empty())
        .unwrap_or(false)
}

pub(crate) fn session_for_generation(gen: u32) -> Option<ReactiveSession> {
    let tag = (gen >> SESSION_TAG_SHIFT) as usize;
    if tag == 0 {
        return None;
    }
    THREAD_SESSIONS
        .try_with(|table| {
            let tbl = table.try_borrow().ok()?;
            let inner = tbl.slots.get(tag - 1)?.weak.upgrade()?;
            Some(ReactiveSession { inner })
        })
        .ok()
        .flatten()
}

pub(crate) fn active_session() -> ReactiveSession {
    SESSION_STACK.with(|stack| {
        stack
            .borrow()
            .last()
            .cloned()
            .unwrap_or_else(|| DEFAULT_SESSION.with(Clone::clone))
    })
}

pub(crate) fn try_with_active_session<R>(f: impl FnOnce(&ReactiveSession) -> R) -> Option<R> {
    SESSION_STACK
        .try_with(|stack| {
            let explicit = stack.borrow().last().cloned();
            if let Some(sess) = explicit {
                Some(f(&sess))
            } else {
                DEFAULT_SESSION.try_with(f).ok()
            }
        })
        .ok()
        .flatten()
}

/// Returns the [`RuntimeId`] of the currently active reactive session on this thread.
pub fn current_runtime_id() -> RuntimeId {
    active_session().id()
}

/// Returns a thread-safe [`SessionWakeHandle`] for the currently active reactive session.
pub fn current_wake_handle() -> SessionWakeHandle {
    active_session().wake_handle()
}

pub(crate) fn flush_deferred_scope_disposals() {
    let _ = try_with_active_session(flush_session_disposals);
}

pub(crate) fn flush_session_disposals(sess: &ReactiveSession) {
    loop {
        let pending: Vec<ScopeId> = {
            let mut list = sess.inner.pending_disposals.borrow_mut();
            if list.is_empty() {
                break;
            }
            std::mem::take(&mut *list)
        };
        let Ok(mut rt) = sess.inner.runtime.try_borrow_mut() else {
            sess.inner.pending_disposals.borrow_mut().extend(pending);
            break;
        };
        for scope_id in pending {
            rt.dispose_scope_recursive(scope_id);
        }
        drop(rt);
        sess.enter(crate::runtime::drain_deferred_drops_only);
    }
}

pub(crate) struct ActiveRuntimeProxy;

impl ActiveRuntimeProxy {
    pub(crate) fn with<R>(&self, f: impl FnOnce(&RefCell<ReactiveRuntime>) -> R) -> R {
        let sess = active_session();
        f(&sess.inner.runtime)
    }

    pub(crate) fn try_with<R>(
        &self,
        f: impl FnOnce(&RefCell<ReactiveRuntime>) -> R,
    ) -> Result<R, ()> {
        try_with_active_session(|sess| f(&sess.inner.runtime)).ok_or(())
    }
}

pub(crate) static RUNTIME: ActiveRuntimeProxy = ActiveRuntimeProxy;

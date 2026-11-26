//! Dedicated non-blocking background flush worker separating SPI `DIRTYFB` ioctl latency
//! from the KRui reactive event/render thread.

use crate::drm_sink::{encode_damage_rect, DrmPixelFormat, DrmPresentSink};
use krui_style::DamageRect;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Instant;

struct PendingFrame {
    fb_w: u32,
    fb_h: u32,
    rgba8: Vec<u8>,
    commits: Vec<DamageRect>,
    seq: u64,
}

struct SharedState {
    pending: Option<PendingFrame>,
    recycled_rgba: Vec<u8>,
    last_completed_seq: u64,
    last_flush_us: u64,
    total_commits: u64,
    last_error: Option<String>,
    shutdown: bool,
}

/// Statistics snapshot reported by [`FlushWorker`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FlushWorkerStats {
    /// Highest frame sequence number completed on the background thread.
    pub completed_seq: u64,
    /// Total number of `DIRTYFB` commit calls flushed across all frames.
    pub total_commits: u64,
    /// Elapsed microseconds measured for the most recent SPI/DRM commit pass.
    pub last_flush_us: u64,
}

/// Non-blocking SPI/DRM flush worker that coalesces superseded frames under heavy load.
pub struct FlushWorker {
    shared: Arc<(Mutex<SharedState>, Condvar)>,
    next_seq: u64,
    handle: Option<JoinHandle<()>>,
}

impl FlushWorker {
    /// Spawns a dedicated flush thread owning `sink` and `format`.
    pub fn spawn<S>(mut sink: S, format: DrmPixelFormat) -> Self
    where
        S: DrmPresentSink + Send + 'static,
    {
        let shared = Arc::new((
            Mutex::new(SharedState {
                pending: None,
                recycled_rgba: Vec::new(),
                last_completed_seq: 0,
                last_flush_us: 0,
                total_commits: 0,
                last_error: None,
                shutdown: false,
            }),
            Condvar::new(),
        ));
        let worker_shared = Arc::clone(&shared);
        let handle = thread::Builder::new()
            .name("krui-kms-flush".to_string())
            .spawn(move || run_worker_loop(&worker_shared, &mut sink, format))
            .expect("spawn krui-kms flush worker");
        Self {
            shared,
            next_seq: 0,
            handle: Some(handle),
        }
    }

    /// Queues a frame snapshot for non-blocking background submission, returning its sequence id.
    ///
    /// If the worker is still flushing a previous frame, pending commit rects are unioned/appended
    /// and the latest framebuffer replaces the older unconsumed snapshot without blocking UI work.
    pub fn submit_frame(
        &mut self,
        rgba8: &[u8],
        fb_w: u32,
        fb_h: u32,
        commits: &[DamageRect],
    ) -> u64 {
        if commits.is_empty() {
            return self.next_seq;
        }
        self.next_seq += 1;
        let seq = self.next_seq;
        let (lock, cvar) = &*self.shared;
        let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        let mut buf = if let Some(prev) = guard.pending.take() {
            let mut merged = prev.commits;
            merged.extend_from_slice(commits);
            let mut reusable = prev.rgba8;
            reusable.clear();
            reusable.extend_from_slice(rgba8);
            guard.pending = Some(PendingFrame {
                fb_w,
                fb_h,
                rgba8: reusable,
                commits: merged,
                seq,
            });
            cvar.notify_one();
            return seq;
        } else {
            std::mem::take(&mut guard.recycled_rgba)
        };
        buf.clear();
        buf.extend_from_slice(rgba8);
        guard.pending = Some(PendingFrame {
            fb_w,
            fb_h,
            rgba8: buf,
            commits: commits.to_vec(),
            seq,
        });
        cvar.notify_one();
        seq
    }

    /// Waits until all queued frames up to `target_seq` have finished flushing.
    pub fn wait_until_flushed(&self, target_seq: u64) -> FlushWorkerStats {
        let (lock, cvar) = &*self.shared;
        let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        while guard.last_completed_seq < target_seq && !guard.shutdown {
            guard = cvar.wait(guard).unwrap_or_else(|e| e.into_inner());
        }
        FlushWorkerStats {
            completed_seq: guard.last_completed_seq,
            total_commits: guard.total_commits,
            last_flush_us: guard.last_flush_us,
        }
    }

    /// Returns the most recent sink error, if any occurred on the background thread.
    pub fn last_error(&self) -> Option<String> {
        let (lock, _) = &*self.shared;
        lock.lock()
            .unwrap_or_else(|e| e.into_inner())
            .last_error
            .clone()
    }
}

impl Drop for FlushWorker {
    fn drop(&mut self) {
        {
            let (lock, cvar) = &*self.shared;
            let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            guard.shutdown = true;
            cvar.notify_all();
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn run_worker_loop<S: DrmPresentSink>(
    shared: &Arc<(Mutex<SharedState>, Condvar)>,
    sink: &mut S,
    format: DrmPixelFormat,
) {
    let (lock, cvar) = &**shared;
    let mut scratch = Vec::new();
    loop {
        let frame = {
            let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
            while guard.pending.is_none() && !guard.shutdown {
                guard = cvar.wait(guard).unwrap_or_else(|e| e.into_inner());
            }
            if guard.shutdown && guard.pending.is_none() {
                break;
            }
            guard.pending.take()
        };
        let Some(frame) = frame else { continue };
        let start = Instant::now();
        let mut ok_commits = 0_u64;
        let mut err_msg = None;
        for rect in &frame.commits {
            let written = encode_damage_rect(
                &frame.rgba8,
                frame.fb_w,
                frame.fb_h,
                *rect,
                format,
                &mut scratch,
            );
            if let Err(err) = sink.commit_damage(*rect, format, &scratch[..written]) {
                err_msg = Some(err);
                break;
            }
            ok_commits += 1;
        }
        let elapsed_us = start.elapsed().as_micros() as u64;
        let mut guard = lock.lock().unwrap_or_else(|e| e.into_inner());
        guard.recycled_rgba = frame.rgba8;
        guard.last_completed_seq = guard.last_completed_seq.max(frame.seq);
        guard.last_flush_us = elapsed_us;
        guard.total_commits = guard.total_commits.saturating_add(ok_commits);
        if let Some(err) = err_msg {
            guard.last_error = Some(err);
        }
        cvar.notify_all();
    }
}

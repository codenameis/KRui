//! Linux DRM DumbBuffer (`RGB565` / `XRGB8888`) + `DIRTYFB` union-bbox commit planner,
//! non-blocking flush thread, and `evdev` kiosk host for small embedded SPI TFT panels.

#![forbid(unsafe_code)]

pub mod commit_planner;
pub mod drm_sink;
pub mod evdev_input;
pub mod flush_worker;
pub mod host;

pub use commit_planner::{plan_dirtyfb_commits, DrmCommitPlan, UNION_BLOAT_SPLIT_RATIO_X10};
pub use drm_sink::{
    encode_damage_rect, DrmFrameCommit, DrmPixelFormat, DrmPresentSink, MockDrmSink,
};
pub use evdev_input::{EvdevInputEvent, KioskInputRouter};
pub use flush_worker::{FlushWorker, FlushWorkerStats};
pub use host::{BootStageTimings, KioskAppHost, KioskFrameReport, KioskHostConfig};

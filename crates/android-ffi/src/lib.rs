//! Safe JNI + Kotlin host bridge and lifecycle controller for KRui on Android (`#![deny(unsafe_code)]`,
//! with 5 `#[no_mangle]` C-ABI export attributes in `jni_entrypoints` and zero `unsafe {}` blocks).

#![deny(unsafe_code)]

pub mod ime_bridge;
pub mod jni_entrypoints;
pub mod preview_client;
pub mod session;
pub mod wear_host;

pub use ime_bridge::{
    composing_cursor_bytes, compute_utf8_delete_surrounding, editor_action_from_android_code,
    input_kind_to_android_code, touch_phase_from_android_action, AndroidTextInputHost,
};
pub use preview_client::AndroidPreviewBridge;
pub use session::{
    create_registered_session, destroy_registered_session, register_session,
    with_registered_session, AndroidSession,
};
pub use wear_host::{WearDisplayMode, WearHostConfig, WearHostState, WearRenderBackend};

//! Verifies `AsyncRuntimeMode::ExternalHandle` reuses a host tokio runtime without extra workers.

#![forbid(unsafe_code)]

use krui_signals::{init_async_runtime_mode, tokio_handle, AsyncRuntimeMode};

#[test]
fn external_handle_mode_reuses_host_runtime_and_rejects_second_init() {
    let ext = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build external current_thread runtime");
    let expected_id = ext.handle().id();

    init_async_runtime_mode(AsyncRuntimeMode::ExternalHandle(ext.handle().clone()))
        .expect("first init_async_runtime_mode must succeed");
    assert!(
        init_async_runtime_mode(AsyncRuntimeMode::SingleWorkerKiosk).is_err(),
        "second init_async_runtime_mode must return Err"
    );
    assert_eq!(tokio_handle().id(), expected_id);

    let done = ext.block_on(async { tokio_handle().spawn(async { 42_u32 }).await.unwrap() });
    assert_eq!(done, 42);
}

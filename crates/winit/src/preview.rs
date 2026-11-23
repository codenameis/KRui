//! # `krui::preview` (`#[cfg(feature = "preview")]`)
//!
//! Re-exports the `krui-preview` runtime `CompiledDocument` interpreter, `TemplateExpr` evaluator,
//! structural-path state preservation, and `DevFileWatcher`.
//! Compiled **only** when the `preview` Cargo feature is enabled so release builds
//! contain zero symbols from `krui_compiler` or `krui_preview`.

use crate::window_helpers::KruiWinitEvent;
pub use krui_preview::{
    compile_krui_source, compile_krui_source_from_path, compile_krui_source_from_path_with_options,
    compile_krui_source_strict, compile_krui_source_with_options, decode_compiled_document,
    decode_rle_rgba_frame, decorate_preview_paint_scene, encode_compiled_document,
    encode_compiled_document_with_version, encode_rle_rgba_frame, eval_interpolated_parts,
    eval_template_expr, eval_truthy, format_structural_key, hot_patch_background_color,
    hot_patch_text_literal, mount_compiled_document, mount_compiled_document_with_registry,
    reload_compiled_document_with_registry, resolve_krui_path, resolve_signal_from_expr,
    try_eval_bool, try_eval_template_expr, try_reload_compiled_document_with_registry,
    validate_compiled_document_against_registry, validate_template_expr, write_length_prefixed,
    CompileError, CompileOptions, CompileWarning, CompileWarningKind, CompiledDocument,
    DevFileWatcher, DeviceFramePreset, EvalError, ForItemScopeRegistry, FramedTcpConnection,
    InspectedNodeWire, IrCodecError, PanelFrameState, PanelInputEvent, PreviewCommand,
    PreviewOverlayFlags, PreviewOverlayKind, PreviewResponse, PreviewTcpClient, PreviewTcpServer,
    StructuralPathKey, VersionedIrBundle, DEFAULT_PREVIEW_PORT, FRAME_RLE_MAGIC, IR_FORMAT_VERSION,
    IR_WIRE_MAGIC, KRUI_DEVICE_FRAME_ENV, KRUI_EMBED_PANEL_ENV, KRUI_SERVE_ADDR_ENV,
    PREVIEW_PROTOCOL_VERSION,
};
use winit::event_loop::EventLoopProxy;

pub(crate) fn init_window_hot_reloader(
    path: Option<&str>,
    proxy: &EventLoopProxy<KruiWinitEvent>,
) -> Option<DevFileWatcher> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let mut watcher = path.and_then(DevFileWatcher::new);
    if let Some(w) = &mut watcher {
        let proxy_for_worker = proxy.clone();
        if w.has_tcp_client() {
            w.start_tcp_wakeup_listener(move || {
                let _ = proxy_for_worker.send_event(KruiWinitEvent::WakeUi);
            });
        } else {
            w.start_background_worker(move || {
                let _ = proxy_for_worker.send_event(KruiWinitEvent::WakeUi);
            });
        }
    }
    watcher
}

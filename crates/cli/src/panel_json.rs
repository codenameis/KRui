//! JSON status, diagnostic, and `stateSync` formatters for `krui serve --panel`.

use krui_compiler::CompileError;
use krui_preview::PanelFrameState;

/// Formats `{"kind":"building",...}` status JSON while the preview binary compiles.
pub fn format_building_json(target: &str, message: &str) -> String {
    format!(
        r#"{{"kind":"building","target":"{}","message":"{}"}}"#,
        json_escape(target),
        json_escape(message)
    )
}

/// Formats `{"kind":"stateSync",...}` JSON describing the current engine frame and IME focus state.
pub fn format_state_sync_json(state: &PanelFrameState) -> String {
    let frame_json = state.device_frame_code.as_deref().map_or_else(
        || "null".to_string(),
        |s| format!(r#""{}""#, json_escape(s)),
    );
    let caret_json = match state.caret_rect {
        Some((x, y, w, h)) => format!(
            "[{:.2},{:.2},{:.2},{:.2}]",
            finite_f32(x),
            finite_f32(y),
            finite_f32(w),
            finite_f32(h)
        ),
        None => "null".to_string(),
    };
    format!(
        concat!(
            r#"{{"kind":"stateSync","seq":{},"width":{},"height":{},"backend":"{}","#,
            r#""theme":"{}","deviceFrame":{},"overlays":{{"relayout":{},"fps":{},"hit":{}}},"#,
            r#""nodeCount":{},"relayoutPasses":{},"reloadCount":{},"frameUs":{},"#,
            r#""cursor":"{}","imeFocused":{},"isPassword":{},"caretRect":{}}}"#
        ),
        state.seq,
        state.width,
        state.height,
        json_escape(&state.backend_name),
        json_escape(&state.theme_code),
        frame_json,
        state.overlays.relayout,
        state.overlays.fps,
        state.overlays.hit,
        state.node_count,
        state.relayout_passes,
        state.reload_count,
        state.frame_us,
        json_escape(&state.cursor_kind),
        state.ime_focused,
        state.is_password,
        caret_json
    )
}

/// Formats `{"kind":"compileOk",...}` JSON after clean `.krui`/`.css` compilation.
pub fn format_compile_ok_json(file: &str, bytes: usize) -> String {
    format!(
        r#"{{"kind":"compileOk","file":"{}","bytes":{}}}"#,
        json_escape(file),
        bytes
    )
}

/// Formats `{"kind":"compileError",...}` JSON with diagnostic file, line, column, and message.
pub fn format_compile_error_json(err: &CompileError, fallback_file: &str) -> String {
    let file = err
        .file
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| fallback_file.to_string());
    format!(
        r#"{{"kind":"compileError","file":"{}","line":{},"column":{},"message":"{}"}}"#,
        json_escape(&file),
        err.line,
        err.column,
        json_escape(&err.message)
    )
}

/// Formats `{"kind":"jumpToSource",...}` JSON when inspecting a widget in the preview canvas.
pub fn format_jump_to_source_json(
    file: &str,
    line: usize,
    column: usize,
    tag: &str,
    id: Option<&str>,
) -> String {
    let id_json = id.map_or_else(
        || "null".to_string(),
        |v| format!(r#""{}""#, json_escape(v)),
    );
    format!(
        r#"{{"kind":"jumpToSource","file":"{}","line":{},"column":{},"tag":"{}","id":{}}}"#,
        json_escape(file),
        line,
        column,
        json_escape(tag),
        id_json
    )
}

/// Escapes `raw` for safe inclusion inside a JSON string literal.
pub fn json_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        match ch {
            '\\' => out.push_str(r"\\"),
            '"' => out.push_str(r#"\""#),
            '\n' => out.push_str(r"\n"),
            '\r' => out.push_str(r"\r"),
            '\t' => out.push_str(r"\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}

fn finite_f32(v: f32) -> f32 {
    if v.is_finite() {
        v
    } else {
        0.0
    }
}

//! Minimal 7-symbol JNI C-ABI boundary (`0` `unsafe {}` blocks, `7` `#[no_mangle]` attributes).

use crate::session::{
    create_registered_session, destroy_registered_session, with_registered_session,
};
use crate::widget_host::{
    create_registered_widget_host, destroy_registered_widget_host, with_registered_widget_host,
};
use jni::objects::{JClass, JIntArray, JString};
use jni::sys::{jboolean, jfloat, jint, jlong, JNI_FALSE, JNI_TRUE};
use jni::JNIEnv;
use std::panic::{catch_unwind, AssertUnwindSafe};

const fn to_jbool(value: bool) -> jboolean {
    if value {
        JNI_TRUE
    } else {
        JNI_FALSE
    }
}

/// Guards an `extern "system"` JNI boundary so a Rust panic never unwinds into the JVM (preventing UB).
pub fn catch_ffi_panic<R>(fallback: R, f: impl FnOnce() -> R) -> R {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(fallback)
}

fn read_jstring(env: &mut JNIEnv<'_>, text: &JString<'_>) -> Option<String> {
    if text.is_null() {
        return None;
    }
    env.get_string(text).ok().map(Into::into)
}

fn dispatch_session_op(
    op: jint,
    session_id: jlong,
    a: jint,
    b: jint,
    scale: jfloat,
    flag: jboolean,
) -> jlong {
    match op {
        0 => create_registered_session(a, b, scale, flag != JNI_FALSE),
        1 => jlong::from(destroy_registered_session(session_id)),
        2 => dispatch_bool_session(session_id, |s| {
            s.on_surface_created(a.max(1) as u32, b.max(1) as u32, scale)
        }),
        3 => dispatch_bool_session(session_id, |s| {
            s.on_surface_resized(a.max(1) as u32, b.max(1) as u32, scale)
        }),
        4 => dispatch_bool_session(session_id, |s| s.on_surface_destroyed()),
        5 => dispatch_bool_session(session_id, |s| s.step_frame(scale)),
        6 => dispatch_bool_session(session_id, |s| s.poll_needs_frame()),
        7 => dispatch_bool_session(session_id, |s| s.should_show_keyboard()),
        8 => with_registered_session(session_id, |s| i64::from(s.input_kind_code())).unwrap_or(0),
        9 => dispatch_bool_session(session_id, |s| s.finish_composing_text()),
        10 => dispatch_bool_session(session_id, |s| s.perform_editor_action(a)),
        11 => with_registered_session(session_id, |s| s.focused_node_key()).unwrap_or(0),
        12 => with_registered_session(session_id, |s| s.packed_framebuffer_size()).unwrap_or(0),
        _ => 0,
    }
}

/// Dispatches lifecycle, surface, frame, keyboard query, focus-identity, framebuffer-size,
/// finish-composing, and editor-action ops for `KruiBridge.nativeSessionOp`.
// SAFETY: Unique Java JNI symbol name `Java_dev_krui_android_KruiBridge_nativeSessionOp` matching
// `KruiBridge.nativeSessionOp(IJIIFZ)J`; catches panics and contains zero `unsafe` blocks.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "system" fn Java_dev_krui_android_KruiBridge_nativeSessionOp(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    op: jint,
    session_id: jlong,
    a: jint,
    b: jint,
    scale: jfloat,
    flag: jboolean,
) -> jlong {
    catch_ffi_panic(0, || dispatch_session_op(op, session_id, a, b, scale, flag))
}

fn dispatch_bool_session(
    session_id: jlong,
    f: impl FnOnce(&mut crate::session::AndroidSession) -> bool,
) -> jlong {
    jlong::from(with_registered_session(session_id, f).unwrap_or(false))
}

/// Dispatches touch (`is_insets == 0`) or window-insets (`is_insets != 0`) updates for `KruiBridge`.
// SAFETY: Unique Java JNI symbol name `Java_dev_krui_android_KruiBridge_nativeMotionOrInsets` matching
// `KruiBridge.nativeMotionOrInsets(ZJJIFFFFF)Z`; catches panics and contains zero `unsafe` blocks.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "system" fn Java_dev_krui_android_KruiBridge_nativeMotionOrInsets(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    is_insets: jboolean,
    session_id: jlong,
    pointer_id: jlong,
    action: jint,
    f0: jfloat,
    f1: jfloat,
    f2: jfloat,
    f3: jfloat,
    f4: jfloat,
) -> jboolean {
    catch_ffi_panic(JNI_FALSE, || {
        let ok = with_registered_session(session_id, |s| {
            if is_insets != JNI_FALSE {
                s.on_insets_changed(f0, f1, f2, f3, f4)
            } else {
                s.on_touch(pointer_id as u64, action, f0, f1, f2)
            }
        })
        .unwrap_or(false);
        to_jbool(ok)
    })
}

/// Dispatches UTF-8 `JString` composing (`op = 0`), commit (`op = 1`), or preview-connect (`op = 2`).
// SAFETY: Unique Java JNI symbol name `Java_dev_krui_android_KruiBridge_nativeTextOp` matching
// `KruiBridge.nativeTextOp(IJLjava/lang/String;I)Z`; checks null, catches panics, and uses safe `jni::JNIEnv::get_string`.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "system" fn Java_dev_krui_android_KruiBridge_nativeTextOp(
    mut env: JNIEnv<'_>,
    _class: JClass<'_>,
    op: jint,
    session_id: jlong,
    text: JString<'_>,
    cursor_pos: jint,
) -> jboolean {
    catch_ffi_panic(JNI_FALSE, || {
        let Some(s_text) = read_jstring(&mut env, &text) else {
            return JNI_FALSE;
        };
        let ok = with_registered_session(session_id, |s| match op {
            0 => s.set_composing_text(&s_text, cursor_pos),
            1 => s.commit_text(&s_text, cursor_pos),
            2 => s.connect_preview(&s_text),
            _ => false,
        })
        .unwrap_or(false);
        to_jbool(ok)
    })
}

/// Dispatches UTF-16 `deleteSurroundingText` (`is_selection == 0`) or `setSelection` (`is_selection != 0`).
// SAFETY: Unique Java JNI symbol name `Java_dev_krui_android_KruiBridge_nativeImeRangeOp` matching
// `KruiBridge.nativeImeRangeOp(ZJII)Z`; catches panics and operates strictly on integer handles and primitive args.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "system" fn Java_dev_krui_android_KruiBridge_nativeImeRangeOp(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    is_selection: jboolean,
    session_id: jlong,
    a: jint,
    b: jint,
) -> jboolean {
    catch_ffi_panic(JNI_FALSE, || {
        let ok = with_registered_session(session_id, |s| {
            if is_selection != JNI_FALSE {
                s.set_selection_utf16(a, b)
            } else {
                s.delete_surrounding_text_utf16(a, b)
            }
        })
        .unwrap_or(false);
        to_jbool(ok)
    })
}

/// Copies the latest rendered `ARGB_8888` framebuffer into `out_argb` using safe `JNIEnv::set_int_array_region`.
// SAFETY: Unique Java JNI symbol name `Java_dev_krui_android_KruiBridge_nativeCopyFramebuffer` matching
// `KruiBridge.nativeCopyFramebuffer(J[I)Z`; checks null/bounds, catches panics, and contains zero `unsafe` blocks.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "system" fn Java_dev_krui_android_KruiBridge_nativeCopyFramebuffer(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    session_id: jlong,
    out_argb: JIntArray<'_>,
) -> jboolean {
    catch_ffi_panic(JNI_FALSE, || {
        if out_argb.is_null() {
            return JNI_FALSE;
        }
        let Ok(dst_len) = env.get_array_length(&out_argb) else {
            return JNI_FALSE;
        };
        let copied = with_registered_session(session_id, |s| {
            s.with_framebuffer_argb(|_w, _h, pixels| {
                if (dst_len as usize) < pixels.len() {
                    return false;
                }
                env.set_int_array_region(&out_argb, 0, pixels).is_ok()
            })
            .unwrap_or(false)
        })
        .unwrap_or(false);
        to_jbool(copied)
    })
}

fn dispatch_widget_op(
    op: jint,
    widget_id: jlong,
    a: jint,
    b: jint,
    scale: jfloat,
    flag: jboolean,
) -> jlong {
    let dark = flag != JNI_FALSE;
    match op {
        0 => create_registered_widget_host(a, b, scale, dark),
        1 => jlong::from(destroy_registered_widget_host(widget_id)),
        2 => jlong::from(
            with_registered_widget_host(widget_id, |w| {
                w.resize(a.max(1) as u32, b.max(1) as u32, scale, dark)
            })
            .unwrap_or(false),
        ),
        3 => jlong::from(
            with_registered_widget_host(widget_id, |w| w.render_snapshot().is_some())
                .unwrap_or(false),
        ),
        4 => jlong::from(
            with_registered_widget_host(widget_id, |w| w.dispatch_action(a.max(0) as u32))
                .unwrap_or(false),
        ),
        5 => with_registered_widget_host(widget_id, |w| w.packed_snapshot_size()).unwrap_or(0),
        6 => with_registered_widget_host(widget_id, |w| w.hotspot_count() as jlong).unwrap_or(0),
        7 => with_registered_widget_host(widget_id, |w| w.revision() as jlong).unwrap_or(0),
        _ => 0,
    }
}

/// Dispatches home-screen widget host lifecycle, resize, render, and tap-action ops for
/// `KruiBridge.nativeWidgetOp`.
// SAFETY: Unique Java JNI symbol name `Java_dev_krui_android_KruiBridge_nativeWidgetOp` matching
// `KruiBridge.nativeWidgetOp(IJIIFZ)J`; catches panics and contains zero `unsafe` blocks.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "system" fn Java_dev_krui_android_KruiBridge_nativeWidgetOp(
    _env: JNIEnv<'_>,
    _class: JClass<'_>,
    op: jint,
    widget_id: jlong,
    a: jint,
    b: jint,
    scale: jfloat,
    flag: jboolean,
) -> jlong {
    catch_ffi_panic(0, || dispatch_widget_op(op, widget_id, a, b, scale, flag))
}

/// Copies the rendered `ARGB_8888` widget bitmap (`is_hotspots == 0`) or packed hotspot tuples
/// `[actionId, x, y, w, h, ...]` (`is_hotspots != 0`) into `out_ints`.
// SAFETY: Unique Java JNI symbol name `Java_dev_krui_android_KruiBridge_nativeCopyWidgetData` matching
// `KruiBridge.nativeCopyWidgetData(JZ[I)Z`; checks null/bounds, catches panics, zero `unsafe` blocks.
#[allow(unsafe_code)]
#[no_mangle]
pub extern "system" fn Java_dev_krui_android_KruiBridge_nativeCopyWidgetData(
    env: JNIEnv<'_>,
    _class: JClass<'_>,
    widget_id: jlong,
    is_hotspots: jboolean,
    out_ints: JIntArray<'_>,
) -> jboolean {
    catch_ffi_panic(JNI_FALSE, || {
        if out_ints.is_null() {
            return JNI_FALSE;
        }
        let Ok(dst_len) = env.get_array_length(&out_ints) else {
            return JNI_FALSE;
        };
        let copied = with_registered_widget_host(widget_id, |w| {
            if is_hotspots != JNI_FALSE {
                w.with_packed_hotspots(|buf| copy_checked_ints(&env, &out_ints, dst_len, buf))
                    .unwrap_or(false)
            } else {
                w.with_snapshot_argb(|_w, _h, px| copy_checked_ints(&env, &out_ints, dst_len, px))
                    .unwrap_or(false)
            }
        })
        .unwrap_or(false);
        to_jbool(copied)
    })
}

fn copy_checked_ints(
    env: &JNIEnv<'_>,
    out_ints: &JIntArray<'_>,
    dst_len: jint,
    src: &[i32],
) -> bool {
    if (dst_len.max(0) as usize) < src.len() {
        return false;
    }
    env.set_int_array_region(out_ints, 0, src).is_ok()
}

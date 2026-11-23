//! Minimal 5-symbol JNI C-ABI boundary (`0` `unsafe {}` blocks, `5` `#[no_mangle]` attributes).

use crate::session::{
    create_registered_session, destroy_registered_session, with_registered_session,
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

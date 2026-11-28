# `krui-android-ffi`

Android JNI + Kotlin 호스트 브리지 크레이트입니다(`AGENTS.md` §1.1 유일한 플랫폼 연결 예외 크레이트).

- 크레이트 루트에 `#![deny(unsafe_code)]`를 유지하며 **직접 작성한 `unsafe {}` 블록은 0줄**입니다.
- 모든 세션 포인터는 원시 포인터(`*mut AndroidSession`) 대신 스레드 로컬 정수 핸들 테이블(`SESSIONS: HashMap<i64, AndroidSession>`)로 격리하며, 모든 `extern "system"` JNI 경계는 `catch_unwind`로 패닉 전파(UB)를 차단합니다.

## unsafe 목록

| 파일:라인 | 항목 | 줄 수 | 대안 검토 및 안전성 근거 (`// SAFETY:`) |
|---|---|---|---|
| `src/jni_entrypoints.rs:64` | `#[no_mangle]` (`Java_dev_krui_android_KruiBridge_nativeSessionOp`) | 1 | `jni = "0.21"` safe 래퍼를 사용하며 JVM 정적 심볼 바인딩을 위해 `#[no_mangle]`만 사용. 모든 생명주기·서피스·프레임·포커스 조회를 단일 심볼로 다중화. |
| `src/jni_entrypoints.rs:88` | `#[no_mangle]` (`Java_dev_krui_android_KruiBridge_nativeMotionOrInsets`) | 1 | 멀티터치(`MotionEvent`)와 `WindowInsetsCompat` 업데이트를 정수 핸들 + 원시 실수 인자만으로 안전하게 전달. |
| `src/jni_entrypoints.rs:120` | `#[no_mangle]` (`Java_dev_krui_android_KruiBridge_nativeTextOp`) | 1 | `JString::is_null()` 검사 후 `JNIEnv::get_string` safe API만 사용하여 IME 조합/확정 및 라이브 프리뷰 주소를 전달. |
| `src/jni_entrypoints.rs:146` | `#[no_mangle]` (`Java_dev_krui_android_KruiBridge_nativeImeRangeOp`) | 1 | UTF-16 `deleteSurroundingText` 및 `setSelection` 범위를 정수 핸들과 원시 정수 인자만으로 전달. |
| `src/jni_entrypoints.rs:177` | `#[no_mangle]` (`Java_dev_krui_android_KruiBridge_nativeCopyFramebuffer`) | 1 | `JIntArray::is_null()` 및 길이 검사 후 `JNIEnv::set_int_array_region` safe API만 사용하여 `ARGB_8888` 프레임버퍼를 복사. |
| `src/jni_entrypoints.rs:247` | `#[no_mangle]` (`Java_dev_krui_android_KruiBridge_nativeWidgetOp`) | 1 | 홈 화면 위젯 호스트 생성/해제/리사이즈/스냅샷 렌더링/탭 액션 디스패치를 단일 정수 핸들 심볼로 다중화 (`unsafe` 블록 0줄). |
| `src/jni_entrypoints.rs:266` | `#[no_mangle]` (`Java_dev_krui_android_KruiBridge_nativeCopyWidgetData`) | 1 | `JIntArray::is_null()` 및 길이 검사 후 `JNIEnv::set_int_array_region`으로 `ARGB_8888` 비트맵 또는 패킹된 핫스팟 튜플을 복사. |
| **합계** | **`unsafe {}` 블록 0줄 / `#[no_mangle]` 7줄** | **7 / 10** | UniFFI 대비 추가 런타임 의존성 없이 `jni` 크레이트의 safe API만으로 `0` `unsafe {}` 블록 달성. |

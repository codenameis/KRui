package dev.krui.android

/**
 * Safe JNI entrypoints exported by `libkrui_android_ffi.so` (`krui-android-ffi`),
 * multiplexing 21 Kotlin bridge calls over 5 consolidated C-ABI symbols.
 */
object KruiBridge {
    init {
        System.loadLibrary("krui_android_ffi")
    }

    const val INPUT_KIND_TEXT: Int = 0
    const val INPUT_KIND_PASSWORD: Int = 1
    const val INPUT_KIND_NUMBER: Int = 2
    const val INPUT_KIND_EMAIL: Int = 3
    const val INPUT_KIND_URL: Int = 4
    const val INPUT_KIND_SEARCH: Int = 5
    const val INPUT_KIND_PHONE: Int = 6
    const val INPUT_KIND_MULTILINE: Int = 7

    private const val OP_CREATE: Int = 0
    private const val OP_DESTROY: Int = 1
    private const val OP_SURFACE_CREATED: Int = 2
    private const val OP_SURFACE_RESIZED: Int = 3
    private const val OP_SURFACE_DESTROYED: Int = 4
    private const val OP_STEP_FRAME: Int = 5
    private const val OP_POLL_NEEDS_FRAME: Int = 6
    private const val OP_SHOULD_SHOW_KEYBOARD: Int = 7
    private const val OP_GET_INPUT_KIND: Int = 8
    private const val OP_FINISH_COMPOSING: Int = 9
    private const val OP_PERFORM_EDITOR_ACTION: Int = 10
    private const val OP_GET_FOCUSED_NODE_KEY: Int = 11
    private const val OP_GET_FRAMEBUFFER_SIZE: Int = 12

    private const val TEXT_OP_COMPOSING: Int = 0
    private const val TEXT_OP_COMMIT: Int = 1
    private const val TEXT_OP_CONNECT_PREVIEW: Int = 2

    @JvmStatic
    private external fun nativeSessionOp(
        op: Int,
        sessionId: Long,
        a: Int,
        b: Int,
        scale: Float,
        flag: Boolean,
    ): Long

    @JvmStatic
    private external fun nativeMotionOrInsets(
        isInsets: Boolean,
        sessionId: Long,
        pointerId: Long,
        action: Int,
        f0: Float,
        f1: Float,
        f2: Float,
        f3: Float,
        f4: Float,
    ): Boolean

    @JvmStatic
    private external fun nativeTextOp(
        op: Int,
        sessionId: Long,
        text: String,
        cursorPos: Int,
    ): Boolean

    @JvmStatic
    private external fun nativeImeRangeOp(
        isSelection: Boolean,
        sessionId: Long,
        a: Int,
        b: Int,
    ): Boolean

    @JvmStatic
    external fun nativeCopyFramebuffer(
        sessionId: Long,
        outArgb: IntArray,
    ): Boolean

    @JvmStatic
    fun nativeCreateSession(
        widthPx: Int,
        heightPx: Int,
        scaleFactor: Float,
        preferGpu: Boolean,
    ): Long = nativeSessionOp(OP_CREATE, 0L, widthPx, heightPx, scaleFactor, preferGpu)

    @JvmStatic
    fun nativeDestroySession(sessionId: Long) {
        nativeSessionOp(OP_DESTROY, sessionId, 0, 0, 0f, false)
    }

    @JvmStatic
    fun nativeOnSurfaceCreated(
        sessionId: Long,
        widthPx: Int,
        heightPx: Int,
        scaleFactor: Float,
    ): Boolean = nativeSessionOp(OP_SURFACE_CREATED, sessionId, widthPx, heightPx, scaleFactor, false) != 0L

    @JvmStatic
    fun nativeOnSurfaceResized(
        sessionId: Long,
        widthPx: Int,
        heightPx: Int,
        scaleFactor: Float,
    ): Boolean = nativeSessionOp(OP_SURFACE_RESIZED, sessionId, widthPx, heightPx, scaleFactor, false) != 0L

    @JvmStatic
    fun nativeOnSurfaceDestroyed(sessionId: Long): Boolean =
        nativeSessionOp(OP_SURFACE_DESTROYED, sessionId, 0, 0, 0f, false) != 0L

    @JvmStatic
    fun nativeOnTouch(
        sessionId: Long,
        pointerId: Long,
        actionMasked: Int,
        xPx: Float,
        yPx: Float,
        dtSeconds: Float,
    ): Boolean = nativeMotionOrInsets(false, sessionId, pointerId, actionMasked, xPx, yPx, dtSeconds, 0f, 0f)

    @JvmStatic
    fun nativeOnInsetsChanged(
        sessionId: Long,
        topPx: Float,
        rightPx: Float,
        bottomPx: Float,
        leftPx: Float,
        imeBottomPx: Float,
    ): Boolean = nativeMotionOrInsets(true, sessionId, 0L, 0, topPx, rightPx, bottomPx, leftPx, imeBottomPx)

    @JvmStatic
    fun nativeStepFrame(sessionId: Long, dtSeconds: Float): Boolean =
        nativeSessionOp(OP_STEP_FRAME, sessionId, 0, 0, dtSeconds, false) != 0L

    @JvmStatic
    fun nativePollNeedsFrame(sessionId: Long): Boolean =
        nativeSessionOp(OP_POLL_NEEDS_FRAME, sessionId, 0, 0, 0f, false) != 0L

    @JvmStatic
    fun nativeShouldShowKeyboard(sessionId: Long): Boolean =
        nativeSessionOp(OP_SHOULD_SHOW_KEYBOARD, sessionId, 0, 0, 0f, false) != 0L

    @JvmStatic
    fun nativeGetInputKind(sessionId: Long): Int =
        nativeSessionOp(OP_GET_INPUT_KIND, sessionId, 0, 0, 0f, false).toInt()

    @JvmStatic
    fun nativeGetFocusedNodeKey(sessionId: Long): Long =
        nativeSessionOp(OP_GET_FOCUSED_NODE_KEY, sessionId, 0, 0, 0f, false)

    @JvmStatic
    fun nativeGetFramebufferSize(sessionId: Long): Long =
        nativeSessionOp(OP_GET_FRAMEBUFFER_SIZE, sessionId, 0, 0, 0f, false)

    @JvmStatic
    fun nativeSetComposingText(
        sessionId: Long,
        text: String,
        newCursorPosition: Int,
    ): Boolean = nativeTextOp(TEXT_OP_COMPOSING, sessionId, text, newCursorPosition)

    @JvmStatic
    fun nativeCommitText(
        sessionId: Long,
        text: String,
        newCursorPosition: Int,
    ): Boolean = nativeTextOp(TEXT_OP_COMMIT, sessionId, text, newCursorPosition)

    @JvmStatic
    fun nativeDeleteSurroundingText(
        sessionId: Long,
        beforeLength: Int,
        afterLength: Int,
    ): Boolean = nativeImeRangeOp(false, sessionId, beforeLength, afterLength)

    @JvmStatic
    fun nativeSetSelection(
        sessionId: Long,
        start: Int,
        end: Int,
    ): Boolean = nativeImeRangeOp(true, sessionId, start, end)

    @JvmStatic
    fun nativeFinishComposingText(sessionId: Long): Boolean =
        nativeSessionOp(OP_FINISH_COMPOSING, sessionId, 0, 0, 0f, false) != 0L

    @JvmStatic
    fun nativePerformEditorAction(sessionId: Long, imeAction: Int): Boolean =
        nativeSessionOp(OP_PERFORM_EDITOR_ACTION, sessionId, imeAction, 0, 0f, false) != 0L

    @JvmStatic
    fun nativeConnectPreview(sessionId: Long, addr: String): Boolean =
        nativeTextOp(TEXT_OP_CONNECT_PREVIEW, sessionId, addr, 0)
}

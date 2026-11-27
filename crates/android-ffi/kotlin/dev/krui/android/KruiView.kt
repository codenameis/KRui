package dev.krui.android

import android.content.Context
import android.graphics.Bitmap
import android.text.InputType
import android.util.AttributeSet
import android.view.Choreographer
import android.view.MotionEvent
import android.view.SurfaceHolder
import android.view.SurfaceView
import android.view.View
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputConnection
import android.view.inputmethod.InputMethodManager
import androidx.core.view.ViewCompat
import androidx.core.view.WindowInsetsCompat
import kotlin.math.max

/**
 * Android host view bridging [SurfaceView], [Choreographer], `WindowInsetsCompat`,
 * multi-touch gestures, framebuffer blitting, and [InputConnection] IME composition to KRui via [KruiBridge].
 */
class KruiView @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : SurfaceView(context, attrs), SurfaceHolder.Callback, Choreographer.FrameCallback {

    private var sessionId: Long = 0L
    private var surfaceReady: Boolean = false
    private var frameScheduled: Boolean = false
    private var lastFrameNanos: Long = 0L
    private var lastTouchNanos: Long = 0L
    private var activeFocusKey: Long = 0L
    private var activeInputKind: Int = -1
    private var frameBitmap: Bitmap? = null
    private var pixelBuffer: IntArray = IntArray(0)

    init {
        val density = max(resources.displayMetrics.density, 1f)
        sessionId = KruiBridge.nativeCreateSession(
            max(width, 1),
            max(height, 1),
            density,
            true,
        )
        isFocusable = true
        isFocusableInTouchMode = true
        holder.addCallback(this)
        ViewCompat.setOnApplyWindowInsetsListener(this) { _, insets ->
            val sys = insets.getInsets(WindowInsetsCompat.Type.systemBars())
            val ime = insets.getInsets(WindowInsetsCompat.Type.ime())
            val imeBottom = max(0, ime.bottom - sys.bottom).toFloat()
            if (sessionId != 0L && KruiBridge.nativeOnInsetsChanged(
                    sessionId,
                    sys.top.toFloat(),
                    sys.right.toFloat(),
                    sys.bottom.toFloat(),
                    sys.left.toFloat(),
                    imeBottom,
                )
            ) {
                scheduleFrame()
            }
            insets
        }
    }

    /** Connects this view to a `krui serve --android` host (no-op unless built with `preview`). */
    fun connectLivePreview(addr: String = "127.0.0.1:41731"): Boolean {
        return sessionId != 0L && KruiBridge.nativeConnectPreview(sessionId, addr)
    }

    override fun surfaceCreated(holder: SurfaceHolder) {
        if (sessionId == 0L) return
        surfaceReady = true
        val frame = holder.surfaceFrame
        val density = max(resources.displayMetrics.density, 1f)
        KruiBridge.nativeOnSurfaceCreated(
            sessionId,
            max(frame.width(), 1),
            max(frame.height(), 1),
            density,
        )
        scheduleFrame()
    }

    override fun surfaceChanged(holder: SurfaceHolder, format: Int, width: Int, height: Int) {
        if (sessionId == 0L) return
        val density = max(resources.displayMetrics.density, 1f)
        KruiBridge.nativeOnSurfaceResized(sessionId, max(width, 1), max(height, 1), density)
        scheduleFrame()
    }

    override fun surfaceDestroyed(holder: SurfaceHolder) {
        surfaceReady = false
        frameBitmap?.recycle()
        frameBitmap = null
        if (sessionId != 0L) {
            KruiBridge.nativeOnSurfaceDestroyed(sessionId)
        }
        frameScheduled = false
        Choreographer.getInstance().removeFrameCallback(this)
    }

    override fun onDetachedFromWindow() {
        dispose()
        super.onDetachedFromWindow()
    }

    override fun onTouchEvent(event: MotionEvent): Boolean {
        if (sessionId == 0L) return super.onTouchEvent(event)
        val now = System.nanoTime()
        val dt = if (lastTouchNanos == 0L) 0.016f else ((now - lastTouchNanos) / 1e9f).coerceIn(0.0001f, 0.05f)
        lastTouchNanos = now
        val action = event.actionMasked
        var handled = false
        if (action == MotionEvent.ACTION_MOVE || action == MotionEvent.ACTION_CANCEL) {
            for (i in 0 until event.pointerCount) {
                val pointerDt = if (i == 0) dt else 0f
                handled = KruiBridge.nativeOnTouch(
                    sessionId,
                    event.getPointerId(i).toLong(),
                    action,
                    event.getX(i),
                    event.getY(i),
                    pointerDt,
                ) || handled
            }
        } else {
            val idx = event.actionIndex
            handled = KruiBridge.nativeOnTouch(
                sessionId,
                event.getPointerId(idx).toLong(),
                action,
                event.getX(idx),
                event.getY(idx),
                dt,
            )
        }
        syncSoftKeyboardVisibility()
        if (handled) {
            scheduleFrame()
        }
        return handled || super.onTouchEvent(event)
    }

    override fun onCheckIsTextEditor(): Boolean {
        return sessionId != 0L && KruiBridge.nativeShouldShowKeyboard(sessionId)
    }

    override fun onCreateInputConnection(outAttrs: EditorInfo): InputConnection? {
        if (sessionId == 0L || !KruiBridge.nativeShouldShowKeyboard(sessionId)) {
            return null
        }
        val kind = KruiBridge.nativeGetInputKind(sessionId)
        outAttrs.inputType = when (kind) {
            KruiBridge.INPUT_KIND_PASSWORD ->
                InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
            KruiBridge.INPUT_KIND_NUMBER ->
                InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_FLAG_DECIMAL
            KruiBridge.INPUT_KIND_EMAIL ->
                InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS
            KruiBridge.INPUT_KIND_URL ->
                InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI
            KruiBridge.INPUT_KIND_SEARCH -> InputType.TYPE_CLASS_TEXT
            KruiBridge.INPUT_KIND_PHONE -> InputType.TYPE_CLASS_PHONE
            KruiBridge.INPUT_KIND_MULTILINE ->
                InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE
            else -> InputType.TYPE_CLASS_TEXT
        }
        val actionFlag = if (kind == KruiBridge.INPUT_KIND_SEARCH) {
            EditorInfo.IME_ACTION_SEARCH
        } else {
            EditorInfo.IME_ACTION_DONE
        }
        outAttrs.imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI or actionFlag
        return KruiInputConnection(this, sessionId) { scheduleFrame() }
    }

    override fun doFrame(frameTimeNanos: Long) {
        frameScheduled = false
        if (sessionId == 0L) return
        val dt = if (lastFrameNanos == 0L) 1f / 60f else ((frameTimeNanos - lastFrameNanos) / 1e9f).coerceIn(0.001f, 0.05f)
        lastFrameNanos = frameTimeNanos
        KruiBridge.nativeStepFrame(sessionId, dt)
        presentFrame()
        syncSoftKeyboardVisibility()
        if (KruiBridge.nativePollNeedsFrame(sessionId)) {
            scheduleFrame()
        }
    }

    fun scheduleFrame() {
        if (!frameScheduled && sessionId != 0L) {
            frameScheduled = true
            Choreographer.getInstance().postFrameCallback(this)
        }
    }

    fun dispose() {
        surfaceReady = false
        frameBitmap?.recycle()
        frameBitmap = null
        if (sessionId != 0L) {
            val id = sessionId
            sessionId = 0L
            Choreographer.getInstance().removeFrameCallback(this)
            KruiBridge.nativeDestroySession(id)
        }
    }

    private fun presentFrame() {
        if (!surfaceReady || sessionId == 0L) return
        val packedSize = KruiBridge.nativeGetFramebufferSize(sessionId)
        val fbW = (packedSize ushr 32).toInt()
        val fbH = (packedSize and 0xFFFF_FFFFL).toInt()
        if (fbW <= 0 || fbH <= 0) return
        val pixelCount = fbW * fbH
        if (pixelBuffer.size < pixelCount) {
            pixelBuffer = IntArray(pixelCount)
        }
        var bmp = frameBitmap
        if (bmp == null || bmp.width != fbW || bmp.height != fbH) {
            bmp?.recycle()
            bmp = Bitmap.createBitmap(fbW, fbH, Bitmap.Config.ARGB_8888)
            frameBitmap = bmp
        }
        if (!KruiBridge.nativeCopyFramebuffer(sessionId, pixelBuffer)) return
        bmp.setPixels(pixelBuffer, 0, fbW, 0, 0, fbW, fbH)
        val canvas = holder.lockCanvas() ?: return
        try {
            canvas.drawBitmap(bmp, null, holder.surfaceFrame, null)
        } finally {
            holder.unlockCanvasAndPost(canvas)
        }
    }

    private fun syncSoftKeyboardVisibility() {
        val imm = context.getSystemService(Context.INPUT_METHOD_SERVICE) as? InputMethodManager ?: return
        if (KruiBridge.nativeShouldShowKeyboard(sessionId)) {
            val focusKey = KruiBridge.nativeGetFocusedNodeKey(sessionId)
            val inputKind = KruiBridge.nativeGetInputKind(sessionId)
            val focusChanged = activeFocusKey != 0L && (focusKey != activeFocusKey || inputKind != activeInputKind)
            activeFocusKey = focusKey
            activeInputKind = inputKind
            requestFocus()
            if (focusChanged) {
                imm.restartInput(this)
            }
            imm.showSoftInput(this, InputMethodManager.SHOW_IMPLICIT)
        } else {
            activeFocusKey = 0L
            activeInputKind = -1
            imm.hideSoftInputFromWindow(windowToken, 0)
        }
    }
}

private class KruiInputConnection(
    targetView: View,
    private val sessionId: Long,
    private val onDirty: () -> Unit,
) : BaseInputConnection(targetView, true) {

    override fun setComposingText(text: CharSequence?, newCursorPosition: Int): Boolean {
        val changed = KruiBridge.nativeSetComposingText(sessionId, text?.toString() ?: "", newCursorPosition)
        if (changed) onDirty()
        return true
    }

    override fun commitText(text: CharSequence?, newCursorPosition: Int): Boolean {
        val changed = KruiBridge.nativeCommitText(sessionId, text?.toString() ?: "", newCursorPosition)
        if (changed) onDirty()
        return true
    }

    override fun deleteSurroundingText(beforeLength: Int, afterLength: Int): Boolean {
        val changed = KruiBridge.nativeDeleteSurroundingText(sessionId, beforeLength, afterLength)
        if (changed) onDirty()
        return true
    }

    override fun setSelection(start: Int, end: Int): Boolean {
        val changed = KruiBridge.nativeSetSelection(sessionId, start, end)
        if (changed) onDirty()
        return true
    }

    override fun finishComposingText(): Boolean {
        val changed = KruiBridge.nativeFinishComposingText(sessionId)
        if (changed) onDirty()
        return true
    }

    override fun performEditorAction(editorAction: Int): Boolean {
        val changed = KruiBridge.nativePerformEditorAction(sessionId, editorAction)
        if (changed) onDirty()
        return true
    }
}

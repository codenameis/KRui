package dev.krui.android

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.Context
import android.content.Intent
import android.content.res.Configuration
import android.graphics.Bitmap
import android.os.Bundle
import android.widget.FrameLayout
import android.widget.ImageView
import android.widget.RemoteViews
import java.util.concurrent.ConcurrentHashMap
import kotlin.math.max
import kotlin.math.roundToInt

/**
 * Android home-screen `AppWidgetProvider` rendering KRui widgets offscreen via `TinySkiaRenderer`
 * into an `ARGB_8888` `Bitmap` + hotspot `PendingIntent` overlay inside `RemoteViews`.
 *
 * Host apps supply an XML layout with a root `FrameLayout` (`containerViewId`) and an `ImageView`
 * (`imageViewId`) plus clickable overlay slot IDs (`overlaySlotViewIds`), or subclass and
 * override those IDs to match their `res/layout/` widget XML.
 */
open class KruiWidgetProvider(
    private val layoutResId: Int,
    private val containerViewId: Int,
    private val imageViewId: Int,
    private val overlaySlotViewIds: IntArray = intArrayOf(),
) : AppWidgetProvider() {

    companion object {
        const val ACTION_KRUI_WIDGET_CLICK: String = "dev.krui.android.ACTION_WIDGET_CLICK"
        const val EXTRA_WIDGET_ID: String = "krui_app_widget_id"
        const val EXTRA_ACTION_ID: String = "krui_widget_action_id"

        private val widgetHandles = ConcurrentHashMap<Int, Long>()
    }

    override fun onUpdate(
        context: Context,
        appWidgetManager: AppWidgetManager,
        appWidgetIds: IntArray,
    ) {
        for (appWidgetId in appWidgetIds) {
            val options = appWidgetManager.getAppWidgetOptions(appWidgetId)
            renderAndPushWidget(context, appWidgetManager, appWidgetId, options)
        }
    }

    override fun onAppWidgetOptionsChanged(
        context: Context,
        appWidgetManager: AppWidgetManager,
        appWidgetId: Int,
        newOptions: Bundle,
    ) {
        renderAndPushWidget(context, appWidgetManager, appWidgetId, newOptions)
    }

    override fun onReceive(context: Context, intent: Intent) {
        super.onReceive(context, intent)
        if (intent.action == ACTION_KRUI_WIDGET_CLICK) {
            val appWidgetId = intent.getIntExtra(
                EXTRA_WIDGET_ID,
                AppWidgetManager.INVALID_APPWIDGET_ID,
            )
            val actionId = intent.getIntExtra(EXTRA_ACTION_ID, -1)
            val handle = widgetHandles[appWidgetId]
            if (handle != null && handle > 0L && actionId >= 0) {
                if (KruiBridge.nativeDispatchWidgetAction(handle, actionId)) {
                    val manager = AppWidgetManager.getInstance(context)
                    val options = manager.getAppWidgetOptions(appWidgetId)
                    pushCurrentSnapshot(context, manager, appWidgetId, handle, options)
                }
            }
        }
    }

    override fun onDeleted(context: Context, appWidgetIds: IntArray) {
        for (appWidgetId in appWidgetIds) {
            widgetHandles.remove(appWidgetId)?.let { handle ->
                if (handle > 0L) {
                    KruiBridge.nativeDestroyWidget(handle)
                }
            }
        }
    }

    private fun renderAndPushWidget(
        context: Context,
        manager: AppWidgetManager,
        appWidgetId: Int,
        options: Bundle,
    ) {
        val density = max(1f, context.resources.displayMetrics.density)
        val widthDp = max(120, options.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_WIDTH, 320))
        val heightDp = max(80, options.getInt(AppWidgetManager.OPTION_APPWIDGET_MIN_HEIGHT, 160))
        val widthPx = (widthDp * density).roundToInt().coerceAtLeast(1)
        val heightPx = (heightDp * density).roundToInt().coerceAtLeast(1)
        val darkMode = isSystemDarkMode(context)

        val handle = ensureWidgetHandle(appWidgetId, widthPx, heightPx, density, darkMode)
        if (handle <= 0L) return
        KruiBridge.nativeResizeWidget(handle, widthPx, heightPx, density, darkMode)
        if (!KruiBridge.nativeRenderWidget(handle)) return
        pushCurrentSnapshot(context, manager, appWidgetId, handle, options)
    }

    private fun pushCurrentSnapshot(
        context: Context,
        manager: AppWidgetManager,
        appWidgetId: Int,
        handle: Long,
        @Suppress("UNUSED_PARAMETER") options: Bundle,
    ) {
        val packedSize = KruiBridge.nativeGetWidgetSize(handle)
        val widthPx = (packedSize ushr 32).toInt()
        val heightPx = (packedSize and 0xFFFF_FFFFL).toInt()
        if (widthPx <= 0 || heightPx <= 0) return

        val pixels = IntArray(widthPx * heightPx)
        if (!KruiBridge.nativeCopyWidgetPixels(handle, pixels)) return
        val bitmap = Bitmap.createBitmap(pixels, widthPx, heightPx, Bitmap.Config.ARGB_8888)

        val views = RemoteViews(context.packageName, layoutResId)
        views.setImageViewBitmap(imageViewId, bitmap)
        bindHotspotActions(context, views, appWidgetId, handle)
        manager.updateAppWidget(appWidgetId, views)
    }

    private fun bindHotspotActions(
        context: Context,
        views: RemoteViews,
        appWidgetId: Int,
        handle: Long,
    ) {
        val spotCount = KruiBridge.nativeGetWidgetHotspotCount(handle)
        if (spotCount <= 0 || overlaySlotViewIds.isEmpty()) return
        val packed = IntArray(spotCount * 5)
        if (!KruiBridge.nativeCopyWidgetHotspots(handle, packed)) return

        val slotLimit = minOf(spotCount, overlaySlotViewIds.size)
        for (i in 0 until slotLimit) {
            val slotId = overlaySlotViewIds[i]
            val actionId = packed[i * 5]
            val xPx = packed[i * 5 + 1]
            val yPx = packed[i * 5 + 2]
            views.setViewPadding(slotId, xPx, yPx, 0, 0)
            views.setOnClickPendingIntent(
                slotId,
                buildClickIntent(context, appWidgetId, actionId, i),
            )
        }
        // Keep root container accessible as primary fallback click target when slots are omitted.
        val _unused: Int = containerViewId + ImageView.VISIBLE + FrameLayout.VISIBLE
    }

    private fun buildClickIntent(
        context: Context,
        appWidgetId: Int,
        actionId: Int,
        slotIndex: Int,
    ): PendingIntent {
        val clickIntent = Intent(context, javaClass).apply {
            action = ACTION_KRUI_WIDGET_CLICK
            putExtra(EXTRA_WIDGET_ID, appWidgetId)
            putExtra(EXTRA_ACTION_ID, actionId)
        }
        val requestCode = (appWidgetId shl 8) xor slotIndex
        val flags = PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        return PendingIntent.getBroadcast(context, requestCode, clickIntent, flags)
    }

    private fun ensureWidgetHandle(
        appWidgetId: Int,
        widthPx: Int,
        heightPx: Int,
        scaleFactor: Float,
        darkMode: Boolean,
    ): Long = widgetHandles.computeIfAbsent(appWidgetId) {
        KruiBridge.nativeCreateWidget(widthPx, heightPx, scaleFactor, darkMode)
    }

    private fun isSystemDarkMode(context: Context): Boolean {
        val mask = context.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK
        return mask == Configuration.UI_MODE_NIGHT_YES
    }
}

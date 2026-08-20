package com.whatabrowser.wat.webview

import android.graphics.Bitmap
import android.graphics.Canvas
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.View
import android.view.ViewGroup

/**
 * What is behind the glass.
 *
 * Android has no live backdrop blur for an ordinary view, so this does the
 * honest version of it: draw the strip of page that sits behind each bar into a
 * small bitmap, blur it, and hand it to the bar. The cost is kept down by three
 * decisions, in order of how much they save:
 *
 * - **An eighth scale.** A phone-width bar becomes about 135 pixels across. The
 *   blur throws away that detail anyway, so nothing is lost by not capturing it.
 * - **Only the strip.** The canvas is clipped to the bar's own rectangle, so a
 *   long page is not drawn to be thrown away.
 * - **Never during a scroll.** Captures are debounced to after the movement
 *   stops, so the expensive part never happens on a frame that has to be quick.
 *
 * And it watches itself. If a capture takes longer than [BUDGET_MS] the whole
 * thing switches off for the rest of the session and the bars fall back to a flat
 * tint — a phone slow enough to notice this is a phone that should not be paying
 * for it. That is the same reason the previous version had no blur at all; the
 * difference is that this one measures rather than assumes.
 */
class Backdrop(
    private val content: ViewGroup,
    private val bars: List<GlassBar>,
) {

    private val handler = Handler(Looper.getMainLooper())

    private var enabled = true

    private var slowCaptures = 0

    private val refresh = Runnable { captureNow() }

    /** Captures after things have settled, coalescing a burst into one. */
    fun refreshSoon(delayMs: Long = SETTLE_MS) {
        if (!enabled) return
        handler.removeCallbacks(refresh)
        handler.postDelayed(refresh, delayMs)
    }

    fun stop() {
        handler.removeCallbacks(refresh)
    }

    private fun captureNow() {
        if (!enabled || content.width == 0 || content.height == 0) return
        val started = SystemClock.elapsedRealtime()

        for (bar in bars) {
            if (bar.visibility != View.VISIBLE || bar.width == 0 || bar.height == 0) continue
            bar.backdrop = capture(bar)
        }

        val took = SystemClock.elapsedRealtime() - started
        if (took > BUDGET_MS) {
            // One slow frame is a hiccup; three is a device that cannot afford
            // this, and the tint on its own still looks like glass.
            if (++slowCaptures >= 3) {
                enabled = false
                for (bar in bars) bar.backdrop = null
            }
        } else {
            slowCaptures = 0
        }
    }

    private fun capture(bar: GlassBar): Bitmap? {
        val width = bar.width / SCALE
        val height = bar.height / SCALE
        if (width < 1 || height < 1) return null

        // Where this bar sits over the page, in the page container's own
        // coordinates. The bars and the container are siblings, so the offset is
        // the difference between their positions in the window.
        val barPosition = IntArray(2).also(bar::getLocationInWindow)
        val contentPosition = IntArray(2).also(content::getLocationInWindow)
        val offsetX = (barPosition[0] - contentPosition[0]).toFloat()
        val offsetY = (barPosition[1] - contentPosition[1]).toFloat()

        return try {
            val bitmap = Bitmap.createBitmap(width, height, Bitmap.Config.ARGB_8888)
            val canvas = Canvas(bitmap)
            canvas.scale(1f / SCALE, 1f / SCALE)
            canvas.translate(-offsetX, -offsetY)
            // Only the pages are drawn, never the bars: a bar that captured
            // itself would feed its own blur back in, frame after frame.
            content.draw(canvas)

            val pixels = IntArray(width * height)
            bitmap.getPixels(pixels, 0, width, 0, 0, width, height)
            Blur.saturate(pixels, VIBRANCY)
            Blur.blur(pixels, width, height, RADIUS)
            bitmap.setPixels(pixels, 0, width, 0, 0, width, height)
            bitmap
        } catch (_: Throwable) {
            // A WebView that refuses a software draw, or a device out of memory
            // for even a bitmap this size. Either way the bar keeps its tint.
            null
        }
    }

    private companion object {
        /** Captured at an eighth, because a blur cannot tell the difference. */
        const val SCALE = 8

        /** In pixels of the downscaled capture, so about 32 at full size. */
        const val RADIUS = 4

        const val VIBRANCY = 1.35f

        /** Long enough that a fling has finished, short enough to feel attached. */
        const val SETTLE_MS = 140L

        /** Half a frame at 60Hz. Past this, the blur is costing more than it gives. */
        const val BUDGET_MS = 8L
    }
}

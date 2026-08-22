package com.whatabrowser.wat.webview

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.LinearGradient
import android.graphics.Outline
import android.graphics.Paint
import android.graphics.Path
import android.graphics.Rect
import android.graphics.RadialGradient
import android.graphics.RectF
import android.graphics.Shader
import android.os.SystemClock
import android.util.AttributeSet
import android.view.MotionEvent
import android.view.View
import android.view.ViewOutlineProvider
import android.widget.LinearLayout

/**
 * A pane of glass with the page showing through it.
 *
 * The previous version drew a translucent rectangle and called it glass, because
 * Android gives no way to blur live content behind an ordinary view. This one
 * takes the other route: [Backdrop] captures the strip of page behind the bar at
 * an eighth scale, blurs it and hands the result over, and the bar draws that as
 * its own backdrop. Small enough to be cheap, blurred enough that an eighth-scale
 * capture is indistinguishable from a full one — a blur throws that detail away
 * anyway.
 *
 * Over the backdrop, in order, the things that make glass read as a surface
 * rather than as a tinted hole:
 *
 * 1. **Vibrancy.** Colour, lifted back up after the blur averaged it towards
 *    grey. Done in [Blur.saturate] while the capture is still small.
 * 2. **Tint.** The wash that makes text on top readable no matter what is behind.
 * 3. **Sheen.** Bright along the top, gone by a third of the way down. This is
 *    what makes a flat rectangle read as curved.
 * 4. **A lit lower edge**, where light that entered the top comes back out.
 * 5. **A rim** that is brightest at the top and fades around the sides, rather
 *    than a stroke of one colour all the way round.
 * 6. **A soft shadow underneath**, so the glass sits above the page.
 *
 * The corners are [GlassShape]'s continuous curve, not a `cornerRadius`.
 *
 * It is a look-alike built from scratch out of gradients and a blur — not
 * Apple's implementation, which is not published and is not something that could
 * be lifted into an Android view even if it were.
 */
class GlassBar @JvmOverloads constructor(
    context: Context,
    attrs: AttributeSet? = null,
) : LinearLayout(context, attrs) {

    private val palette = GlassSurface.palette(context)

    private val tintPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply { color = palette.glassTint }
    private val sheenPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val glowPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val rimPaint = Paint(Paint.ANTI_ALIAS_FLAG).apply {
        style = Paint.Style.STROKE
        strokeWidth = GlassSurface.dp(context, GlassGeometry.borderWidth).coerceAtLeast(1f)
    }
    private val backdropPaint = Paint(Paint.FILTER_BITMAP_FLAG or Paint.ANTI_ALIAS_FLAG)
    private val fadingPaint = Paint(Paint.FILTER_BITMAP_FLAG or Paint.ANTI_ALIAS_FLAG)
    private val specularPaint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val touchPaint = Paint(Paint.ANTI_ALIAS_FLAG)

    private val bounds = RectF()
    private var shape: Path = Path()

    /** The blurred capture of whatever is behind this bar, or null before one. */
    var backdrop: Bitmap? = null
        set(value) {
            // The one going out is kept for a moment and faded through, so a new
            // capture arrives as a settle rather than as a flicker.
            previous = field
            previousAt = SystemClock.elapsedRealtime()
            field = value
            invalidate()
        }

    private var previous: Bitmap? = null
    private var previousAt = 0L

    /**
     * Where the light is, as -1..1 across the bar.
     *
     * Driven by [Tilt] from the accelerometer, so the highlight slides when the
     * phone turns. This is most of what makes glass read as a surface rather than
     * as a picture of one.
     */
    var light: Float = 0f
        set(value) {
            val clamped = value.coerceIn(-1f, 1f)
            if (kotlin.math.abs(clamped - field) < 0.01f) return
            field = clamped
            buildSpecular()
            invalidate()
        }

    private var touchX = 0f
    private var touchY = 0f
    private var touchAt = 0L

    var cornerRadius: Float = GlassSurface.dp(context, GlassGeometry.radiusLarge)
        set(value) {
            field = value
            rebuild()
            invalidate()
        }

    init {
        // A LinearLayout is a container and skips its own drawing by default.
        setWillNotDraw(false)
        outlineProvider = object : ViewOutlineProvider() {
            override fun getOutline(view: View, outline: Outline) {
                // The outline is what the platform draws the shadow from. It has
                // to be a plain rounded rectangle — an arbitrary path only casts a
                // shadow on Android 10 and later, and this build starts at 7.
                outline.setRoundRect(0, 0, view.width, view.height, cornerRadius)
            }
        }
        elevation = GlassSurface.dp(context, 6f)
    }

    override fun onSizeChanged(width: Int, height: Int, oldWidth: Int, oldHeight: Int) {
        super.onSizeChanged(width, height, oldWidth, oldHeight)
        rebuild()
    }

    private fun rebuild() {
        if (width == 0 || height == 0) return
        bounds.set(0f, 0f, width.toFloat(), height.toFloat())
        shape = GlassShape.path(bounds, cornerRadius)

        val rim = palette.glassRim
        buildSpecular()
        sheenPaint.shader = LinearGradient(
            0f,
            0f,
            0f,
            height * 0.45f,
            withAlpha(rim, 0.22f),
            withAlpha(rim, 0f),
            Shader.TileMode.CLAMP,
        )
        glowPaint.shader = LinearGradient(
            0f,
            height * 0.7f,
            0f,
            height.toFloat(),
            withAlpha(rim, 0f),
            withAlpha(rim, 0.10f),
            Shader.TileMode.CLAMP,
        )
        // Brightest along the top edge and fading down each side: a rim of one
        // flat colour is the giveaway that a surface is a rectangle with a
        // stroke on it.
        rimPaint.shader = LinearGradient(
            0f,
            0f,
            0f,
            height.toFloat(),
            intArrayOf(
                withAlpha(rim, 0.55f),
                withAlpha(palette.glassEdge, 0.65f),
                withAlpha(palette.glassEdge, 0.30f),
            ),
            floatArrayOf(0f, 0.4f, 1f),
            Shader.TileMode.CLAMP,
        )
    }

    /**
     * The moving highlight: a soft band of light across the glass, positioned by
     * how the phone is being held.
     */
    private fun buildSpecular() {
        if (width == 0) return
        val centre = width * (0.5f + light * 0.42f)
        val reach = width * 0.28f
        specularPaint.shader = LinearGradient(
            centre - reach,
            0f,
            centre + reach,
            height.toFloat(),
            intArrayOf(
                withAlpha(palette.glassRim, 0f),
                withAlpha(palette.glassRim, 0.13f),
                withAlpha(palette.glassRim, 0f),
            ),
            floatArrayOf(0f, 0.5f, 1f),
            Shader.TileMode.CLAMP,
        )
    }

    /**
     * Glass that answers a touch.
     *
     * Apple's moves under a finger; this brightens where it was touched and
     * settles back over [TOUCH_FADE_MS]. The event is only observed — it is never
     * consumed — so the buttons underneath behave exactly as they did.
     */
    override fun onInterceptTouchEvent(event: MotionEvent): Boolean {
        if (event.actionMasked == MotionEvent.ACTION_DOWN) {
            touchX = event.x
            touchY = event.y
            touchAt = SystemClock.elapsedRealtime()
            invalidate()
        }
        return super.onInterceptTouchEvent(event)
    }

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        if (width == 0 || height == 0) return

        val save = canvas.save()
        canvas.clipPath(shape)

        val now = SystemClock.elapsedRealtime()
        val fade = ((now - previousAt).toFloat() / CROSSFADE_MS).coerceIn(0f, 1f)

        previous?.let { bitmap ->
            if (fade >= 1f) {
                previous = null
            } else if (!bitmap.isRecycled) {
                fadingPaint.alpha = ((1f - fade) * 255).toInt()
                canvas.drawBitmap(bitmap, Rect(0, 0, bitmap.width, bitmap.height), bounds, fadingPaint)
            }
        }
        backdrop?.let { bitmap ->
            if (!bitmap.isRecycled) {
                backdropPaint.alpha = if (previous == null) 255 else (fade * 255).toInt()
                canvas.drawBitmap(bitmap, Rect(0, 0, bitmap.width, bitmap.height), bounds, backdropPaint)
            }
        }
        canvas.drawRect(bounds, tintPaint)
        canvas.drawRect(bounds, sheenPaint)
        canvas.drawRect(bounds, specularPaint)
        canvas.drawRect(bounds, glowPaint)

        // Where a finger landed, brightening and settling back.
        val sinceTouch = now - touchAt
        if (touchAt > 0L && sinceTouch < TOUCH_FADE_MS) {
            val strength = 1f - sinceTouch.toFloat() / TOUCH_FADE_MS
            val radius = height * (1.2f + (1f - strength))
            touchPaint.shader = RadialGradient(
                touchX,
                touchY,
                radius.coerceAtLeast(1f),
                withAlpha(palette.glassRim, 0.18f * strength),
                withAlpha(palette.glassRim, 0f),
                Shader.TileMode.CLAMP,
            )
            canvas.drawRect(bounds, touchPaint)
        }
        canvas.restoreToCount(save)

        // Another frame is only asked for while something is actually moving.
        if (fade < 1f || (touchAt > 0L && sinceTouch < TOUCH_FADE_MS)) postInvalidateOnAnimation()

        // The rim is drawn last and unclipped, so the stroke is not shaved in
        // half by its own clip.
        canvas.drawPath(shape, rimPaint)
    }

    private companion object {
        /** Long enough to read as a settle, short enough not to look like a lag. */
        const val CROSSFADE_MS = 220f

        const val TOUCH_FADE_MS = 450L
    }

    private fun withAlpha(colour: Int, fraction: Float): Int = Color.argb(
        (255 * fraction).toInt().coerceIn(0, 255),
        Color.red(colour),
        Color.green(colour),
        Color.blue(colour),
    )
}

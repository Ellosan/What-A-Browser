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
import android.graphics.RectF
import android.graphics.Shader
import android.util.AttributeSet
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

    private val bounds = RectF()
    private var shape: Path = Path()

    /** The blurred capture of whatever is behind this bar, or null before one. */
    var backdrop: Bitmap? = null
        set(value) {
            field = value
            invalidate()
        }

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

    override fun onDraw(canvas: Canvas) {
        super.onDraw(canvas)
        if (width == 0 || height == 0) return

        val save = canvas.save()
        canvas.clipPath(shape)

        backdrop?.let { bitmap ->
            if (!bitmap.isRecycled) {
                canvas.drawBitmap(bitmap, Rect(0, 0, bitmap.width, bitmap.height), bounds, backdropPaint)
            }
        }
        canvas.drawRect(bounds, tintPaint)
        canvas.drawRect(bounds, sheenPaint)
        canvas.drawRect(bounds, glowPaint)
        canvas.restoreToCount(save)

        // The rim is drawn last and unclipped, so the stroke is not shaved in
        // half by its own clip.
        canvas.drawPath(shape, rimPaint)
    }

    private fun withAlpha(colour: Int, fraction: Float): Int = Color.argb(
        (255 * fraction).toInt().coerceIn(0, 255),
        Color.red(colour),
        Color.green(colour),
        Color.blue(colour),
    )
}

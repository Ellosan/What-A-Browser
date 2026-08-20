package com.whatabrowser.wat.webview

import android.content.Context
import android.graphics.Color
import android.graphics.drawable.Drawable
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.LayerDrawable
import android.util.TypedValue

/**
 * The Liquid Glass surface, as an Android drawable.
 *
 * The desktop browser draws this with `wat-paint`: a backdrop blur, a tint over
 * it, a top-down sheen and a rim light on the edge. None of that renderer exists
 * here, so the look is rebuilt out of what the platform draws cheaply — a tinted
 * translucent fill, a vertical sheen, and a hairline edge.
 *
 * What is missing is the blur itself, and it is missing on purpose. There is no
 * way on Android to blur *live* content behind an ordinary view: `RenderEffect`
 * blurs the view it is set on, and `Window.setBackgroundBlurRadius` blurs what is
 * behind the window, not what is behind a toolbar inside it. Doing it properly
 * means capturing the WebView to a bitmap and blurring that every frame, which on
 * a 4 GB phone costs more than the effect is worth — and this build exists
 * because launching fast matters more than looking perfect.
 *
 * The colours come from `Glass.kt`, generated from the same TOML the Rust
 * browser reads, so the two do not drift apart.
 */
object GlassSurface {

    fun palette(context: Context): GlassPalette {
        val night = context.resources.configuration.uiMode and
            android.content.res.Configuration.UI_MODE_NIGHT_MASK
        return if (night == android.content.res.Configuration.UI_MODE_NIGHT_YES) GlassDark else GlassLight
    }

    /** A glass panel: tint, sheen, hairline edge. */
    fun panel(context: Context, radiusDp: Float): Drawable {
        val colors = palette(context)
        val radius = dp(context, radiusDp)

        val tint = GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = radius
            setColor(colors.glassTint)
        }

        // The sheen: brighter at the top, gone by the middle. This is what makes
        // a flat translucent rectangle read as a curved surface.
        val sheen = GradientDrawable(
            GradientDrawable.Orientation.TOP_BOTTOM,
            intArrayOf(
                withAlpha(colors.glassRim, 0.16f),
                withAlpha(colors.glassRim, 0.0f),
            ),
        ).apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = radius
        }

        val edge = GradientDrawable().apply {
            shape = GradientDrawable.RECTANGLE
            cornerRadius = radius
            setColor(Color.TRANSPARENT)
            setStroke(dp(context, GlassGeometry.borderWidth).toInt().coerceAtLeast(1), colors.glassEdge)
        }

        return LayerDrawable(arrayOf(tint, sheen, edge))
    }

    /** A pill, for the address bar and the tab counter. */
    fun pill(context: Context): Drawable = panel(context, 999f)

    private fun withAlpha(color: Int, fraction: Float): Int =
        Color.argb((255 * fraction).toInt(), Color.red(color), Color.green(color), Color.blue(color))

    fun dp(context: Context, value: Float): Float = TypedValue.applyDimension(
        TypedValue.COMPLEX_UNIT_DIP,
        value,
        context.resources.displayMetrics,
    )
}

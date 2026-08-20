package com.whatabrowser.wat.webview

import android.graphics.Path
import android.graphics.RectF

/**
 * The corner shape.
 *
 * A rounded rectangle joins a straight edge to a circular arc, and the curvature
 * jumps at the join — the eye reads that jump as a slightly pinched corner. A
 * continuous corner ramps the curvature in instead, which is why Apple's panels
 * look softer than a `cornerRadius` at the same size, and why the difference
 * survives being described as "a rounder rectangle".
 *
 * This is the usual two-cubic approximation of that curve: the corner starts
 * further along the edge than the radius suggests (about 1.53 times it) and the
 * control points are placed so the second derivative lines up where the arc meets
 * the straight run.
 */
object GlassShape {

    /** How far along the edge the corner starts, as a multiple of the radius. */
    private const val EXTENT = 1.5288993f

    private const val C1 = 0.68440944f
    private const val C2 = 0.37651895f
    private const val C3 = 0.09231669f

    fun path(bounds: RectF, radius: Float): Path {
        val path = Path()
        // Never wider than half the shortest side, or the corners cross over and
        // the shape turns inside out.
        val r = radius.coerceAtMost(minOf(bounds.width(), bounds.height()) / 2f / EXTENT)
        if (r <= 0f) {
            path.addRect(bounds, Path.Direction.CW)
            return path
        }

        val extent = r * EXTENT
        val left = bounds.left
        val top = bounds.top
        val right = bounds.right
        val bottom = bounds.bottom

        path.moveTo(left + extent, top)
        path.lineTo(right - extent, top)
        corner(path, right, top, r, +1f, +1f, horizontalFirst = true)
        path.lineTo(right, bottom - extent)
        corner(path, right, bottom, r, +1f, -1f, horizontalFirst = false)
        path.lineTo(left + extent, bottom)
        corner(path, left, bottom, r, -1f, -1f, horizontalFirst = true)
        path.lineTo(left, top + extent)
        corner(path, left, top, r, -1f, +1f, horizontalFirst = false)
        path.close()
        return path
    }

    /**
     * One corner, as two cubics meeting at its midpoint.
     *
     * [signX] and [signY] point into the shape from the corner, so the same
     * arithmetic serves all four.
     */
    private fun corner(
        path: Path,
        cornerX: Float,
        cornerY: Float,
        r: Float,
        signX: Float,
        signY: Float,
        horizontalFirst: Boolean,
    ) {
        val x = { d: Float -> cornerX - signX * d }
        val y = { d: Float -> cornerY + signY * d }

        if (horizontalFirst) {
            path.cubicTo(x(r * C1), y(0f), x(r * C2), y(0f), x(r * C3), y(r * C3))
            path.cubicTo(x(0f), y(r * C2), x(0f), y(r * C1), x(0f), y(r * EXTENT))
        } else {
            path.cubicTo(x(0f), y(r * C1), x(0f), y(r * C2), x(r * C3), y(r * C3))
            path.cubicTo(x(r * C2), y(0f), x(r * C1), y(0f), x(r * EXTENT), y(0f))
        }
    }
}

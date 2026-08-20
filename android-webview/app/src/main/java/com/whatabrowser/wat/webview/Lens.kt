package com.whatabrowser.wat.webview

/**
 * The bend at the edge of the glass.
 *
 * A real pane is thick, and its edges are curved, so what you see through the
 * last few millimetres is not what is behind them — it is squeezed and pulled
 * inward as the light refracts. This is the single strongest cue that a surface
 * is glass rather than a tinted rectangle, and it is what 0.1.2's version was
 * missing: a blur alone reads as frosted plastic.
 *
 * So the backdrop is resampled before it is drawn: pixels near an edge are taken
 * from further inside the image, by an amount that grows towards the edge. It
 * runs on the same eighth-scale capture as the blur, so the whole pass is a few
 * thousand pixels.
 *
 * Pure arithmetic on an array, and tested, because the failure here is not an
 * ugly edge — it is sampling outside the buffer.
 */
object Lens {

    /**
     * @param depth how far in from each edge the bend reaches, in pixels of this
     *   (downscaled) image
     * @param strength how far the sampling is pulled inward at the very edge, as
     *   a fraction of [depth]
     */
    fun refract(pixels: IntArray, width: Int, height: Int, depth: Int, strength: Float = 0.75f) {
        if (width <= 0 || height <= 0 || depth <= 0 || strength <= 0f) return
        if (pixels.size < width * height) return

        val source = pixels.copyOf()
        val maximum = depth * strength

        for (y in 0 until height) {
            val fromTop = y
            val fromBottom = height - 1 - y
            val verticalShift = shift(minOf(fromTop, fromBottom), depth, maximum)
            val towardsBottom = fromTop < fromBottom

            for (x in 0 until width) {
                val fromLeft = x
                val fromRight = width - 1 - x
                val horizontalShift = shift(minOf(fromLeft, fromRight), depth, maximum)
                if (verticalShift == 0 && horizontalShift == 0) continue

                // Inward: the near edge is the one the light bends away from.
                val sampleX = if (fromLeft < fromRight) x + horizontalShift else x - horizontalShift
                val sampleY = if (towardsBottom) y + verticalShift else y - verticalShift

                pixels[y * width + x] = source[
                    sampleY.coerceIn(0, height - 1) * width + sampleX.coerceIn(0, width - 1),
                ]
            }
        }
    }

    /**
     * How far to reach inward, given the distance to the nearest edge.
     *
     * Squared falloff rather than linear: the bend should be invisible where the
     * glass is flat and then tighten quickly, the way a real curved edge does.
     */
    private fun shift(distance: Int, depth: Int, maximum: Float): Int {
        if (distance >= depth) return 0
        val closeness = 1f - distance.toFloat() / depth
        return (closeness * closeness * maximum).toInt()
    }
}

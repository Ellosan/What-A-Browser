package com.whatabrowser.wat.webview

/**
 * The blur behind the glass, and the saturation lift over it.
 *
 * Plain Kotlin over an ARGB array, for two reasons. `RenderEffect` is Android 12
 * and later only, and this browser runs from Android 7; and a blur is arithmetic,
 * so it can be tested off a device — which matters because a blur that reads a
 * row past the end of its buffer does not look wrong, it crashes.
 *
 * Three box passes, which is the usual stand-in for a Gaussian and is what makes
 * the result look soft rather than smeared. It is cheap because it never runs on
 * a large image: the backdrop is captured at an eighth scale, so a phone-width
 * bar is about 135 pixels across before it is blurred.
 */
object Blur {

    /**
     * Blurs in place. [radius] is in pixels of this (already downscaled) image.
     */
    fun blur(pixels: IntArray, width: Int, height: Int, radius: Int) {
        if (width <= 0 || height <= 0 || radius <= 0) return
        if (pixels.size < width * height) return
        val scratch = IntArray(pixels.size)
        repeat(3) {
            boxHorizontal(pixels, scratch, width, height, radius)
            boxVertical(scratch, pixels, width, height, radius)
        }
    }

    private fun boxHorizontal(src: IntArray, dst: IntArray, width: Int, height: Int, radius: Int) {
        for (y in 0 until height) {
            val row = y * width
            for (x in 0 until width) {
                var a = 0
                var r = 0
                var g = 0
                var b = 0
                var count = 0
                // Clamped at the edges rather than wrapped: a blur that wraps
                // pulls the right-hand side of the screen into the left.
                for (offset in -radius..radius) {
                    val sample = x + offset
                    if (sample < 0 || sample >= width) continue
                    val colour = src[row + sample]
                    a += (colour ushr 24) and 0xFF
                    r += (colour ushr 16) and 0xFF
                    g += (colour ushr 8) and 0xFF
                    b += colour and 0xFF
                    count++
                }
                dst[row + x] = pack(a / count, r / count, g / count, b / count)
            }
        }
    }

    private fun boxVertical(src: IntArray, dst: IntArray, width: Int, height: Int, radius: Int) {
        for (x in 0 until width) {
            for (y in 0 until height) {
                var a = 0
                var r = 0
                var g = 0
                var b = 0
                var count = 0
                for (offset in -radius..radius) {
                    val sample = y + offset
                    if (sample < 0 || sample >= height) continue
                    val colour = src[sample * width + x]
                    a += (colour ushr 24) and 0xFF
                    r += (colour ushr 16) and 0xFF
                    g += (colour ushr 8) and 0xFF
                    b += colour and 0xFF
                    count++
                }
                dst[y * width + x] = pack(a / count, r / count, g / count, b / count)
            }
        }
    }

    /**
     * The vibrancy lift: colour behind glass reads flat once it is blurred,
     * because averaging pulls everything towards grey. Pushing each pixel away
     * from its own brightness puts back what the blur took out, and is why a
     * photo behind Apple's glass still looks like that photo.
     *
     * @param amount 1.0 leaves the image alone; 1.4 is a noticeable lift.
     */
    fun saturate(pixels: IntArray, amount: Float) {
        if (amount == 1f) return
        for (i in pixels.indices) {
            val colour = pixels[i]
            val a = (colour ushr 24) and 0xFF
            val r = (colour ushr 16) and 0xFF
            val g = (colour ushr 8) and 0xFF
            val b = colour and 0xFF
            // Rec. 601 luma: the eye weighs green far more than blue, and a
            // straight average turns green foliage grey.
            val luma = 0.299f * r + 0.587f * g + 0.114f * b
            pixels[i] = pack(
                a,
                clamp(luma + (r - luma) * amount),
                clamp(luma + (g - luma) * amount),
                clamp(luma + (b - luma) * amount),
            )
        }
    }

    private fun clamp(value: Float): Int = value.toInt().coerceIn(0, 255)

    private fun pack(a: Int, r: Int, g: Int, b: Int): Int =
        (a shl 24) or (r shl 16) or (g shl 8) or b
}

package com.whatabrowser.wat.webview

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class BlurTest {

    private fun argb(a: Int, r: Int, g: Int, b: Int) = (a shl 24) or (r shl 16) or (g shl 8) or b

    private fun red(colour: Int) = (colour ushr 16) and 0xFF

    private fun alpha(colour: Int) = (colour ushr 24) and 0xFF

    @Test
    fun `a flat image is unchanged by blurring it`() {
        val flat = IntArray(64) { argb(255, 40, 80, 120) }
        Blur.blur(flat, 8, 8, 2)
        assertTrue(flat.all { it == argb(255, 40, 80, 120) })
    }

    @Test
    fun `a bright pixel spreads to its neighbours`() {
        val pixels = IntArray(81) { argb(255, 0, 0, 0) }
        pixels[40] = argb(255, 255, 255, 255) // the middle of a 9x9
        Blur.blur(pixels, 9, 9, 1)

        assertTrue("the centre dims", red(pixels[40]) < 255)
        assertTrue("its neighbour lights up", red(pixels[39]) > 0)
        // Symmetric: the same distance in any direction gives the same value.
        assertEquals(red(pixels[39]), red(pixels[41]))
        assertEquals(red(pixels[31]), red(pixels[49]))
    }

    @Test
    fun `the blur does not wrap around the edges`() {
        // A lit column on the right must not brighten the left, or the blur pulls
        // one side of the screen into the other.
        val width = 10
        val height = 4
        val pixels = IntArray(width * height) { argb(255, 0, 0, 0) }
        for (y in 0 until height) pixels[y * width + 9] = argb(255, 255, 0, 0)
        Blur.blur(pixels, width, height, 2)
        assertEquals(0, red(pixels[0]))
        assertTrue(red(pixels[width - 1]) > 0)
    }

    @Test
    fun `alpha comes through the blur`() {
        val pixels = IntArray(36) { argb(128, 10, 10, 10) }
        Blur.blur(pixels, 6, 6, 2)
        assertTrue(pixels.all { alpha(it) == 128 })
    }

    @Test
    fun `nonsense arguments are refused rather than read out of bounds`() {
        val pixels = IntArray(9) { argb(255, 1, 2, 3) }
        Blur.blur(pixels, 3, 3, 0)
        Blur.blur(pixels, 0, 0, 4)
        Blur.blur(pixels, -1, 3, 4)
        // A size larger than the buffer: the guard here is the one that would
        // otherwise be an out-of-bounds read on a real backdrop capture.
        Blur.blur(pixels, 100, 100, 2)
        assertTrue(pixels.all { it == argb(255, 1, 2, 3) })
    }

    @Test
    fun `saturation pushes colour away from grey and leaves grey alone`() {
        val grey = intArrayOf(argb(255, 128, 128, 128))
        Blur.saturate(grey, 1.5f)
        assertEquals(argb(255, 128, 128, 128), grey[0])

        val colour = intArrayOf(argb(255, 200, 100, 50))
        Blur.saturate(colour, 1.5f)
        assertTrue("the strong channel strengthens", red(colour[0]) > 200)
        assertEquals("alpha is untouched", 255, alpha(colour[0]))
    }

    @Test
    fun `saturation cannot push a channel outside a byte`() {
        val bright = intArrayOf(argb(255, 255, 0, 0), argb(255, 0, 0, 0))
        Blur.saturate(bright, 4f)
        for (colour in bright) {
            assertTrue(red(colour) in 0..255)
            assertEquals(255, alpha(colour))
        }
    }

    @Test
    fun `an amount of one is a no-op`() {
        val pixels = intArrayOf(argb(200, 12, 34, 56))
        Blur.saturate(pixels, 1f)
        assertEquals(argb(200, 12, 34, 56), pixels[0])
    }
}

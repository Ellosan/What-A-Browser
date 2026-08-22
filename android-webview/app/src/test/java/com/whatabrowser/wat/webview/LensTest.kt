package com.whatabrowser.wat.webview

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class LensTest {

    /** Vertical stripes, so a sideways displacement shows up as a value change. */
    private fun stripes(width: Int, height: Int) =
        IntArray(width * height) { index -> (index % width) * 1000 }

    @Test
    fun `the middle of the pane is not bent`() {
        val width = 40
        val height = 40
        val pixels = stripes(width, height)
        val before = pixels.copyOf()
        Lens.refract(pixels, width, height, depth = 6)

        // Well inside the depth on every side: the glass is flat here.
        for (y in 10 until 30) {
            for (x in 10 until 30) {
                assertEquals("($x,$y)", before[y * width + x], pixels[y * width + x])
            }
        }
    }

    @Test
    fun `the edge samples from further inside`() {
        val width = 40
        val height = 8
        val pixels = stripes(width, height)
        val before = pixels.copyOf()
        Lens.refract(pixels, width, height, depth = 8)

        // The leftmost column now shows a stripe from further right.
        assertNotEquals(before[0], pixels[0])
        assertTrue(pixels[0] > before[0])
        // And the rightmost column shows one from further left.
        assertTrue(pixels[width - 1] < before[width - 1])
    }

    @Test
    fun `the bend is strongest at the very edge`() {
        val width = 60
        val height = 4
        val pixels = stripes(width, height)
        val before = pixels.copyOf()
        Lens.refract(pixels, width, height, depth = 10)

        val atEdge = pixels[0] - before[0]
        val twoIn = pixels[2] - before[2]
        val nearlyFlat = pixels[9] - before[9]
        assertTrue("edge $atEdge should exceed inner $twoIn", atEdge > twoIn)
        assertTrue("inner $twoIn should exceed nearly flat $nearlyFlat", twoIn >= nearlyFlat)
    }

    @Test
    fun `nothing is sampled from outside the image`() {
        // Every value in the result has to have come from the input, or the
        // sampling walked off the end of the buffer.
        val width = 16
        val height = 16
        val pixels = stripes(width, height)
        val allowed = pixels.toSet()
        Lens.refract(pixels, width, height, depth = 12, strength = 2f)
        assertTrue(pixels.all { it in allowed })
    }

    @Test
    fun `nonsense arguments leave the image alone`() {
        val pixels = stripes(4, 4)
        val before = pixels.copyOf()
        Lens.refract(pixels, 4, 4, depth = 0)
        Lens.refract(pixels, 4, 4, depth = 2, strength = 0f)
        Lens.refract(pixels, 0, 0, depth = 2)
        Lens.refract(pixels, 100, 100, depth = 2)
        assertArrayEquals(before, pixels)
    }
}

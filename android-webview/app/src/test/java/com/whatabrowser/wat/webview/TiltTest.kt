package com.whatabrowser.wat.webview

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class TiltTest {

    @Test
    fun `gravity becomes a number between minus one and one`() {
        assertEquals(0f, Tilt.normalize(0f), 0.001f)
        assertEquals(1f, Tilt.normalize(9.81f), 0.01f)
        assertEquals(-1f, Tilt.normalize(-9.81f), 0.01f)
        // A phone being shaken reads well past gravity, and the highlight must
        // not fly off the edge because of it.
        assertEquals(1f, Tilt.normalize(40f), 0.001f)
        assertEquals(-1f, Tilt.normalize(-40f), 0.001f)
    }

    @Test
    fun `smoothing approaches the target without overshooting it`() {
        var value = 0f
        repeat(100) { value = Tilt.smooth(value, 1f) }
        assertTrue(value > 0.99f)
        assertTrue(value <= 1f)
    }

    @Test
    fun `one reading moves the highlight only a little`() {
        // Hand tremor is most of an accelerometer reading; following it exactly
        // makes the highlight shake.
        val step = Tilt.smooth(0f, 1f)
        assertTrue(step > 0f)
        assertTrue(step < 0.25f)
    }

    @Test
    fun `a change too small to see is below the threshold`() {
        val tiny = Tilt.smooth(0.5f, 0.5001f) - 0.5f
        assertTrue(tiny < Tilt.THRESHOLD)
    }
}

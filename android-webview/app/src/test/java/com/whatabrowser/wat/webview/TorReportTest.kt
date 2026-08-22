package com.whatabrowser.wat.webview

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TorReportTest {

    @Test
    fun `the report keeps the order it was given`() {
        val text = TorReport.render(
            listOf("mode" to "LION", "android" to "13", "abi" to "arm64-v8a"),
            listOf("+0.0s  starting", "+2.1s  bootstrapped 45%"),
        )
        assertTrue(text.indexOf("mode:") < text.indexOf("android:"))
        assertTrue(text.indexOf("android:") < text.indexOf("abi:"))
        assertTrue(text.contains("bootstrapped 45%"))
    }

    @Test
    fun `an empty log says so rather than looking truncated`() {
        val text = TorReport.render(listOf("mode" to "LION"), emptyList())
        assertTrue(text.contains("(none)"))
    }

    @Test
    fun `a runaway value is shortened, but not so far that it stops explaining`() {
        // The limit was 200, and the first real failure it reported was a library
        // saying exactly what was missing — cut off mid-sentence at the useful
        // part. Long enough to survive now, still bounded.
        val text = TorReport.render(listOf("reason" to "x".repeat(5000)), emptyList())
        assertTrue(text.lines().all { it.length <= 720 })
        assertTrue(text.lines().any { it.length > 300 })
        assertTrue(text.contains("…"))
    }

    @Test
    fun `the whole report fits in something a person can send`() {
        val log = (1..500).map { "event number $it with a good deal of text after it" }
        val text = TorReport.render(listOf("mode" to "LION"), log)
        assertTrue(text.length <= 4000)
        assertTrue(text.endsWith("(trimmed)\n"))
    }

    @Test
    fun `a newline in a value cannot forge a second field`() {
        val text = TorReport.render(listOf("reason" to "first\nmode: NORMAL"), emptyList())
        assertEquals(1, text.lines().count { it.startsWith("reason:") })
        assertFalse(text.lines().any { it.startsWith("mode:") })
    }

    @Test
    fun `nothing appears that was not handed in`() {
        // The window this comes from is the private one; a report that invented a
        // field could invent one with a page address in it.
        val text = TorReport.render(listOf("mode" to "LION"), listOf("started"))
        val expected = setOf("WAT Tor diagnostics", "mode: LION", "", "recent tor events:", "  started")
        assertTrue(text.lines().filter { it.isNotEmpty() }.all { it in expected })
    }
}

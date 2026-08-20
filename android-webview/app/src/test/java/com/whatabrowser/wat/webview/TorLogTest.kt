package com.whatabrowser.wat.webview

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class TorLogTest {

    @Test
    fun `it keeps the newest and drops the oldest`() {
        val log = TorLog(capacity = 3)
        for (line in listOf("one", "two", "three", "four")) log.add(line)
        val lines = log.lines()
        assertEquals(3, lines.size)
        assertTrue(lines[0].endsWith("two"))
        assertTrue(lines[2].endsWith("four"))
    }

    @Test
    fun `every line is stamped with time since the window opened`() {
        // Relative, not wall-clock: a report gets pasted somewhere, and the hour
        // someone was browsing is not part of a tor failure.
        val log = TorLog()
        log.add("starting")
        assertTrue(log.lines().first().startsWith("+"))
        assertTrue(log.lines().first().contains("s  starting"))
    }

    @Test
    fun `clearing empties it`() {
        val log = TorLog()
        log.add("one")
        log.clear()
        assertTrue(log.lines().isEmpty())
    }
}

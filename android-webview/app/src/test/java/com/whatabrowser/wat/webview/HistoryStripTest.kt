package com.whatabrowser.wat.webview

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class HistoryStripTest {

    private val pages = (0..4).map { HistoryStrip.Page("https://$it/", "Page $it") }

    @Test
    fun `back offers the nearest page first, counting down`() {
        val back = HistoryStrip.back(pages, currentIndex = 3)
        assertEquals(listOf("https://2/", "https://1/", "https://0/"), back.map { it.page.url })
        assertEquals(listOf(-1, -2, -3), back.map { it.steps })
    }

    @Test
    fun `forward counts the other way`() {
        val forward = HistoryStrip.forward(pages, currentIndex = 1)
        assertEquals(listOf("https://2/", "https://3/", "https://4/"), forward.map { it.page.url })
        assertEquals(listOf(1, 2, 3), forward.map { it.steps })
    }

    @Test
    fun `the ends of the history offer nothing`() {
        assertTrue(HistoryStrip.back(pages, currentIndex = 0).isEmpty())
        assertTrue(HistoryStrip.forward(pages, currentIndex = 4).isEmpty())
        assertTrue(HistoryStrip.back(emptyList(), currentIndex = 0).isEmpty())
    }

    @Test
    fun `an index outside the list is not trusted`() {
        // A WebView with no page yet reports -1, and a stale index is worse than
        // no menu at all.
        assertTrue(HistoryStrip.back(pages, currentIndex = -1).isEmpty())
        assertTrue(HistoryStrip.forward(pages, currentIndex = 9).isEmpty())
    }

    @Test
    fun `a long history is cut off rather than scrolled forever`() {
        val long = (0..99).map { HistoryStrip.Page("https://$it/", "") }
        val back = HistoryStrip.back(long, currentIndex = 99)
        assertEquals(HistoryStrip.LIMIT, back.size)
        assertEquals(-1, back.first().steps)
        assertEquals(-HistoryStrip.LIMIT, back.last().steps)
    }

    @Test
    fun `a page with no title shows its address`() {
        assertEquals("https://x/", HistoryStrip.Page("https://x/", "").label)
        assertEquals("Title", HistoryStrip.Page("https://x/", "Title").label)
    }
}

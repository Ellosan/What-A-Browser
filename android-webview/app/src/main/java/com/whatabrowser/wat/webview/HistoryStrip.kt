package com.whatabrowser.wat.webview

/**
 * The pages behind and ahead of the one being read.
 *
 * A long press on back or forward should offer the history rather than making
 * the reader tap eleven times to get back to a search result. The WebView keeps
 * that list; this turns it into something to show, and works out how many steps
 * each entry is from here — which is the part that is easy to get off by one and
 * so the part worth testing.
 */
object HistoryStrip {

    class Page(val url: String, val title: String) {
        val label: String get() = title.ifBlank { url }
    }

    /** @param steps what to pass to `goBackOrForward`: negative back, positive forward. */
    class Entry(val steps: Int, val page: Page)

    /** How many entries a long press offers before the list stops being useful. */
    const val LIMIT = 12

    /** Nearest first: the page one back, then the one before it. */
    fun back(pages: List<Page>, currentIndex: Int, limit: Int = LIMIT): List<Entry> {
        if (currentIndex !in pages.indices) return emptyList()
        val first = maxOf(0, currentIndex - limit)
        return (currentIndex - 1 downTo first).map { Entry(it - currentIndex, pages[it]) }
    }

    /** Nearest first, the other way. */
    fun forward(pages: List<Page>, currentIndex: Int, limit: Int = LIMIT): List<Entry> {
        if (currentIndex !in pages.indices) return emptyList()
        val last = minOf(pages.size - 1, currentIndex + limit)
        return (currentIndex + 1..last).map { Entry(it - currentIndex, pages[it]) }
    }
}

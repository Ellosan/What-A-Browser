package com.whatabrowser.wat.webview

/**
 * The last few things tor said before it stopped working.
 *
 * A Tor window that fails shows one line, and one line is rarely enough to tell
 * what went wrong — but a browser cannot keep an unbounded log of what it was
 * doing either. This keeps the most recent [capacity] events and throws the rest
 * away, with times relative to when the window opened rather than wall-clock,
 * since a report is meant to be pasted somewhere and a timestamp says when
 * someone was browsing.
 */
class TorLog(private val capacity: Int = 40) {

    private val lines = ArrayDeque<String>()

    private val started = System.nanoTime()

    fun add(line: String) {
        val elapsed = (System.nanoTime() - started) / 1_000_000
        while (lines.size >= capacity) lines.removeFirst()
        lines.addLast("+%.1fs  %s".format(elapsed / 1000f, line.trim()))
    }

    /** Oldest first, which is the order they are worth reading in. */
    fun lines(): List<String> = lines.toList()

    fun clear() = lines.clear()
}

package com.whatabrowser.wat.webview

/**
 * The tabs, as a model: what is open, which one is in front, and which one has
 * gone longest without being looked at.
 *
 * No `WebView` here on purpose. Tab order and selection are where browsers get
 * fiddly — closing the tab you are on, opening a link "in the background",
 * closing the last one — and all of it is ordinary list bookkeeping that a JVM
 * test can check. [Tabs] owns the views; this owns the rules.
 *
 * The recency order exists because of memory. A live `WebView` is tens of
 * megabytes, and this browser is meant for a phone with 4 GB in it, so only a
 * few tabs keep one; the rest are frozen to a saved state and restored when
 * they come forward. Deciding *which* to freeze is this class's job.
 */
class TabList(private val maxTabs: Int = 16) {

    class Tab(val id: Int, var url: String, var title: String = "") {
        /** What to show in the switcher: the page's title, or its address. */
        val label: String get() = title.ifBlank { url }
    }

    private val tabs = mutableListOf<Tab>()

    /** Most recently selected first. Every open tab appears exactly once. */
    private val recency = mutableListOf<Int>()

    private var nextId = 1

    var currentId: Int? = null
        private set

    val count: Int get() = tabs.size

    val isFull: Boolean get() = tabs.size >= maxTabs

    fun all(): List<Tab> = tabs.toList()

    fun byId(id: Int): Tab? = tabs.firstOrNull { it.id == id }

    val current: Tab? get() = currentId?.let(::byId)

    /**
     * Opens a tab, next to the one it came from.
     *
     * A link opened from a page belongs beside that page, not at the far end of
     * the strip — that is what every desktop browser does and it is the one piece
     * of tab ordering people notice when it is wrong. Returns null when the tab
     * limit is reached, which the caller reports rather than silently ignoring.
     */
    fun open(url: String, foreground: Boolean = true, after: Int? = currentId): Tab? {
        if (isFull) return null
        val tab = Tab(nextId++, url)
        val index = after?.let { id -> tabs.indexOfFirst { it.id == id } }?.takeIf { it >= 0 }
        if (index == null) tabs.add(tab) else tabs.add(index + 1, tab)
        // A background tab is the least recently used thing there is: it has
        // never been looked at.
        recency.add(tab.id)
        if (foreground) select(tab.id)
        return tab
    }

    fun select(id: Int): Tab? {
        val tab = byId(id) ?: return null
        currentId = id
        recency.remove(id)
        recency.add(0, id)
        return tab
    }

    /**
     * Closes a tab and returns whatever should be in front afterwards.
     *
     * Closing the tab you are looking at moves to the one on its right, or the
     * one on its left when it was last. Null means nothing is left, and the
     * caller decides what an empty browser does.
     */
    fun close(id: Int): Tab? {
        val index = tabs.indexOfFirst { it.id == id }
        if (index < 0) return current
        val wasCurrent = id == currentId
        tabs.removeAt(index)
        recency.remove(id)
        if (!wasCurrent) return current

        currentId = null
        val next = tabs.getOrNull(index) ?: tabs.lastOrNull() ?: return null
        return select(next.id)
    }

    fun closeAll() {
        tabs.clear()
        recency.clear()
        currentId = null
    }

    /**
     * Which of [among] to give up first: the one the reader has gone longest
     * without looking at, never the one they are looking at now.
     */
    fun leastRecentlyUsed(among: Collection<Int>): Int? =
        recency.lastOrNull { it in among && it != currentId }

    /** For restoring a saved session: the order and the selection come back too. */
    fun restore(entries: List<Tab>, selected: Int?) {
        closeAll()
        tabs.addAll(entries)
        nextId = (entries.maxOfOrNull { it.id } ?: 0) + 1
        recency.addAll(entries.map { it.id })
        val target = selected?.takeIf { id -> entries.any { it.id == id } } ?: entries.firstOrNull()?.id
        target?.let(::select)
    }
}

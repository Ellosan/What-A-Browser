package com.whatabrowser.wat.webview

/**
 * Which items are in the menu, and in what order.
 *
 * Stored as a single string in the preferences, which is why the parsing has to
 * be forgiving in two specific directions: an id this version does not know
 * about is dropped, and an id this version has but the stored string does not is
 * added at the end. Without the second, every new menu item would be invisible
 * to everyone who had ever opened the customise screen — the classic way a
 * customisable menu quietly stops gaining features.
 */
class MenuLayout private constructor(
    private val order: MutableList<String>,
    private val hidden: MutableSet<String>,
) {

    class Item(val id: String, val visible: Boolean)

    /** Every item, in the reader's order, hidden ones included. */
    fun items(): List<Item> = order.map { Item(it, it !in hidden) }

    /** What the menu actually shows. */
    fun visible(): List<String> = order.filter { it !in hidden }

    fun isVisible(id: String): Boolean = id in order && id !in hidden

    /**
     * Shows or hides an item.
     *
     * [ALWAYS] cannot be hidden: a menu with no way to reach settings is a menu
     * that cannot be put back the way it was.
     */
    fun toggle(id: String) {
        if (id in ALWAYS || id !in order) return
        if (!hidden.remove(id)) hidden.add(id)
    }

    fun moveUp(id: String) = move(id, -1)

    fun moveDown(id: String) = move(id, 1)

    private fun move(id: String, delta: Int) {
        val from = order.indexOf(id)
        if (from < 0) return
        val to = from + delta
        if (to !in order.indices) return
        order.removeAt(from)
        order.add(to, id)
    }

    fun reset() {
        order.clear()
        order.addAll(ALL)
        hidden.clear()
    }

    /** `new_tab,-downloads,settings`: order is the order, a minus means hidden. */
    fun serialize(): String = order.joinToString(",") { if (it in hidden) "-$it" else it }

    companion object {
        /**
         * Every menu item there is, in the order they appear before anyone
         * rearranges them.
         */
        val ALL = listOf(
            "new_tab",
            "private_cat",
            "private_lion",
            "bookmark",
            "bookmarks",
            "history",
            "find",
            "share",
            "downloads",
            "desktop",
            "customize",
            "settings",
        )

        /** Items that may be moved but never hidden. */
        val ALWAYS = setOf("settings", "customize")

        fun parse(stored: String?): MenuLayout {
            val order = mutableListOf<String>()
            val hidden = mutableSetOf<String>()

            stored?.split(',')?.forEach { raw ->
                val entry = raw.trim()
                if (entry.isEmpty()) return@forEach
                val isHidden = entry.startsWith("-")
                val id = if (isHidden) entry.substring(1) else entry
                // An id from a newer version, or a typo in a hand-edited
                // preference: ignored rather than shown as an item that does
                // nothing when tapped.
                if (id !in ALL || id in order) return@forEach
                order.add(id)
                if (isHidden && id !in ALWAYS) hidden.add(id)
            }

            // Anything this version knows about that the stored order predates.
            ALL.filterNot { it in order }.forEach(order::add)

            return MenuLayout(order, hidden)
        }

        fun default(): MenuLayout = parse(null)
    }
}

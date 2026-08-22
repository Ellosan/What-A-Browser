package com.whatabrowser.wat.webview

/**
 * What each menu id means: its label, and whether it applies at all.
 *
 * Separate from [MenuLayout] so that the model stays free of Android and can be
 * tested, and separate from the activity so that adding a menu item is one entry
 * here rather than three edits spread across a 500-line file.
 */
object MenuCatalog {

    fun labelRes(id: String): Int = when (id) {
        "new_tab" -> R.string.new_tab
        "private_cat" -> R.string.private_cat
        "private_lion" -> R.string.private_lion
        "bookmark" -> R.string.add_bookmark
        "bookmarks" -> R.string.bookmarks
        "history" -> R.string.history
        "find" -> R.string.find_in_page
        "share" -> R.string.share_page
        "downloads" -> R.string.downloads
        "script_commands" -> R.string.script_commands
        "desktop" -> R.string.desktop_site
        "customize" -> R.string.customize_menu
        "settings" -> R.string.settings
        else -> R.string.unknown
    }

    /**
     * Whether an item belongs in this window's menu.
     *
     * A private window has no bookmarks and no history — not hidden ones, none:
     * it cannot read the ordinary window's, by design. Offering the item and then
     * doing nothing would be worse than not offering it. And a private window
     * does not open further private windows, which is a way to lose track of how
     * many are open.
     */
    fun appliesTo(id: String, privacy: PrivacyMode, userScripts: Boolean = true): Boolean = when (id) {
        // Nothing to list when scripts are switched off, and a menu item that
        // only ever says "nothing here" is worse than no menu item.
        "script_commands" -> userScripts
        "bookmark", "bookmarks", "history" -> !privacy.isPrivate
        "private_cat", "private_lion" -> !privacy.isPrivate
        // Settings live in the ordinary window. Preferences are one file, and
        // two processes writing it is how a setting silently reverts; a private
        // window reads them and changes nothing.
        "settings", "customize" -> !privacy.isPrivate
        else -> true
    }
}

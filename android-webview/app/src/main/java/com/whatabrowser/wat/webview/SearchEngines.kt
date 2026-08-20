package com.whatabrowser.wat.webview

/**
 * The search engines offered in settings.
 *
 * All HTTPS, and all with a home page of their own so that changing the engine
 * changes where the home button goes as well — an engine you have to type an
 * address to reach is not really the default.
 */
object SearchEngines {

    class Engine(val name: String, val template: String, val home: String)

    val ALL = listOf(
        Engine("DuckDuckGo", "https://duckduckgo.com/?q={}", "https://duckduckgo.com/"),
        Engine("Startpage", "https://www.startpage.com/sp/search?query={}", "https://www.startpage.com/"),
        Engine("Brave", "https://search.brave.com/search?q={}", "https://search.brave.com/"),
        Engine("Wikipedia", "https://en.wikipedia.org/w/index.php?search={}", "https://en.wikipedia.org/"),
        Engine("Google", "https://www.google.com/search?q={}", "https://www.google.com/"),
    )

    val DEFAULT = ALL.first()

    fun byName(name: String?): Engine = ALL.firstOrNull { it.name == name } ?: DEFAULT

    /**
     * A template has to be HTTPS and has to have somewhere to put the terms.
     *
     * Checked rather than assumed because a template is a setting, and a setting
     * that reached `http://` would send every search typed into the address bar
     * across the network in the clear.
     */
    fun isUsableTemplate(template: String): Boolean =
        template.startsWith("https://") && template.contains("{}")
}

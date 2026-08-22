package com.whatabrowser.wat.webview

import android.content.Context

/**
 * Everything that can be changed, and the reasons a few things cannot be.
 *
 * The shape of this follows Chrome's settings, because that is what people know:
 * a search engine, a home page, appearance, accessibility, and a privacy section
 * that is the point of the whole screen. Where Chrome offers a switch this
 * browser can honestly offer, it is here. Where it cannot, [SettingsPanel] says
 * so on screen rather than leaving a gap.
 *
 * Two kinds of thing are deliberately *not* settings:
 *
 * - **What keeps the browser safe.** Certificate errors are never overridable,
 *   cleartext is never allowed, `file://` is never reachable from a page, and
 *   third-party cookies are never accepted. A switch that weakens any of those is
 *   a switch that gets flipped once and forgotten, and then the browser is no
 *   longer the thing it claims to be. Chrome makes several of these optional;
 *   this one does not.
 * - **Things that need a permission.** Location, camera and microphone are
 *   refused for every site, because the app holds none of those permissions and
 *   will not start asking for them.
 */
class Settings(context: Context) {

    private val prefs =
        context.applicationContext.getSharedPreferences("settings", Context.MODE_PRIVATE)

    // --- general ------------------------------------------------------------

    var engine: SearchEngines.Engine
        get() = SearchEngines.byName(prefs.getString(KEY_ENGINE, null))
        set(value) = put(KEY_ENGINE, value.name)

    /** Where the home button goes. Follows the search engine until it is set. */
    var homePage: String
        get() = prefs.getString(KEY_HOME, null)?.takeIf { it.isNotBlank() } ?: engine.home
        set(value) = put(KEY_HOME, value)

    fun resetHomePage() = prefs.edit().remove(KEY_HOME).apply()

    /** Light, dark, or whatever the phone is doing. */
    var theme: ThemeChoice
        get() = ThemeChoice.of(prefs.getString(KEY_THEME, null))
        set(value) = put(KEY_THEME, value.name)

    /** Whether new tabs ask for the desktop site. */
    var desktopSite: Boolean
        get() = prefs.getBoolean(KEY_DESKTOP, false)
        set(value) = put(KEY_DESKTOP, value)

    /** The menu's order and visibility, as [MenuLayout] serialises it. */
    var menuLayout: String?
        get() = prefs.getString(KEY_MENU, null)
        set(value) = put(KEY_MENU, value ?: "")

    // --- privacy ------------------------------------------------------------

    /**
     * Google's list of phishing and malware sites.
     *
     * On by default and worth it, but it is a setting because it means the
     * WebView asks Google about sites — Safe Browsing sends a partial hash of the
     * URL — and someone may reasonably weigh that differently.
     */
    var safeBrowsing: Boolean
        get() = prefs.getBoolean(KEY_SAFE_BROWSING, true)
        set(value) = put(KEY_SAFE_BROWSING, value)

    /** Refuse requests to hosts whose only purpose is measurement. */
    var blockTrackers: Boolean
        get() = prefs.getBoolean(KEY_BLOCK_TRACKERS, true)
        set(value) = put(KEY_BLOCK_TRACKERS, value)

    /**
     * Refuse *all* cookies, not only third-party ones.
     *
     * Off by default because it signs you out of everything, which is the same
     * reason Chrome does not offer it prominently. Third-party cookies are
     * blocked either way and that is not optional.
     */
    var blockAllCookies: Boolean
        get() = prefs.getBoolean(KEY_BLOCK_ALL_COOKIES, false)
        set(value) = put(KEY_BLOCK_ALL_COOKIES, value)

    /** Throw away cookies, site storage and history when the browser closes. */
    var clearOnExit: Boolean
        get() = prefs.getBoolean(KEY_CLEAR_ON_EXIT, false)
        set(value) = put(KEY_CLEAR_ON_EXIT, value)

    /** Whether visits are written down at all. */
    var keepHistory: Boolean
        get() = prefs.getBoolean(KEY_HISTORY, true)
        set(value) = put(KEY_HISTORY, value)

    /**
     * Whether the address bar offers what has been visited before.
     *
     * Local only — nothing is ever sent to a suggestion service, which is a
     * thing Chrome cannot say — but it is still the browser showing where you
     * have been to whoever is holding the phone.
     */
    var suggestFromHistory: Boolean
        get() = prefs.getBoolean(KEY_SUGGEST, true)
        set(value) = put(KEY_SUGGEST, value)

    /** Script, for every site. Chrome has this per site; a WebView cannot. */
    var javaScript: Boolean
        get() = prefs.getBoolean(KEY_JAVASCRIPT, true)
        set(value) = put(KEY_JAVASCRIPT, value)

    var images: Boolean
        get() = prefs.getBoolean(KEY_IMAGES, true)
        set(value) = put(KEY_IMAGES, value)

    /** Run installed userscripts. Off until something is installed. */
    var userScripts: Boolean
        get() = prefs.getBoolean(KEY_USERSCRIPTS, false)
        set(value) = put(KEY_USERSCRIPTS, value)

    // --- accessibility ------------------------------------------------------

    /** Percent, as Chrome's text scaling slider works. */
    var textZoom: Int
        get() = prefs.getInt(KEY_TEXT_ZOOM, 100).coerceIn(50, 200)
        set(value) = put(KEY_TEXT_ZOOM, value.coerceIn(50, 200))

    /** Let a page be pinched even when it asks not to be. */
    var forceZoom: Boolean
        get() = prefs.getBoolean(KEY_FORCE_ZOOM, true)
        set(value) = put(KEY_FORCE_ZOOM, value)

    /** Ask the engine to darken sites that have no dark theme of their own. */
    var darkenSites: Boolean
        get() = prefs.getBoolean(KEY_DARKEN, false)
        set(value) = put(KEY_DARKEN, value)

    val searchTemplate: String get() = engine.template

    private fun put(key: String, value: String) = prefs.edit().putString(key, value).apply()

    private fun put(key: String, value: Boolean) = prefs.edit().putBoolean(key, value).apply()

    private fun put(key: String, value: Int) = prefs.edit().putInt(key, value).apply()

    enum class ThemeChoice {
        SYSTEM,
        LIGHT,
        DARK,

        ;

        companion object {
            fun of(name: String?): ThemeChoice =
                entries.firstOrNull { it.name == name } ?: SYSTEM
        }
    }

    private companion object {
        const val KEY_ENGINE = "engine"
        const val KEY_HOME = "home"
        const val KEY_THEME = "theme"
        const val KEY_DESKTOP = "desktop_site"
        const val KEY_MENU = "menu_layout"
        const val KEY_SAFE_BROWSING = "safe_browsing"
        const val KEY_BLOCK_TRACKERS = "block_trackers"
        const val KEY_BLOCK_ALL_COOKIES = "block_all_cookies"
        const val KEY_CLEAR_ON_EXIT = "clear_on_exit"
        const val KEY_HISTORY = "keep_history"
        const val KEY_SUGGEST = "suggest_from_history"
        const val KEY_JAVASCRIPT = "javascript"
        const val KEY_IMAGES = "images"
        const val KEY_USERSCRIPTS = "userscripts"
        const val KEY_TEXT_ZOOM = "text_zoom"
        const val KEY_FORCE_ZOOM = "force_zoom"
        const val KEY_DARKEN = "darken_sites"
    }
}

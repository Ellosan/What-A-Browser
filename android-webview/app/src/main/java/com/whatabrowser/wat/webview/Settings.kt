package com.whatabrowser.wat.webview

import android.content.Context

/**
 * The handful of things worth being able to change.
 *
 * Kept to what an everyday browser actually needs settings for. Everything in
 * [SecureWebView] is deliberately not here: a setting that can turn off mixed
 * content blocking or certificate checking is a setting that will eventually be
 * turned off, and then the browser is no longer the thing it claims to be.
 */
class Settings(context: Context) {

    private val prefs = context.applicationContext.getSharedPreferences("settings", Context.MODE_PRIVATE)

    var engine: SearchEngines.Engine
        get() = SearchEngines.byName(prefs.getString(KEY_ENGINE, null))
        set(value) = prefs.edit().putString(KEY_ENGINE, value.name).apply()

    /** Where the home button goes. Follows the search engine until it is set. */
    var homePage: String
        get() = prefs.getString(KEY_HOME, null)?.takeIf { it.isNotBlank() } ?: engine.home
        set(value) = prefs.edit().putString(KEY_HOME, value).apply()

    /** Whether new pages ask for the desktop site. */
    var desktopSite: Boolean
        get() = prefs.getBoolean(KEY_DESKTOP, false)
        set(value) = prefs.edit().putBoolean(KEY_DESKTOP, value).apply()

    /** Whether visits are written down at all. */
    var keepHistory: Boolean
        get() = prefs.getBoolean(KEY_HISTORY, true)
        set(value) = prefs.edit().putBoolean(KEY_HISTORY, value).apply()

    val searchTemplate: String get() = engine.template

    fun resetHomePage() = prefs.edit().remove(KEY_HOME).apply()

    private companion object {
        const val KEY_ENGINE = "engine"
        const val KEY_HOME = "home"
        const val KEY_DESKTOP = "desktop_site"
        const val KEY_HISTORY = "keep_history"
    }
}

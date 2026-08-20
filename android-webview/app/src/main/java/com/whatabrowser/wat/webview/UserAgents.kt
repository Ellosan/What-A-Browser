package com.whatabrowser.wat.webview

/**
 * The user agent used when a page is asked for as a desktop site.
 *
 * Sites choose their layout from this string, so "request desktop site" is
 * nothing more than sending a different one. It is built from the WebView's own
 * default so the engine version stays truthful — a made-up version number gets
 * served workarounds for bugs this engine does not have.
 *
 * Deliberately not a string with "WAT" in it. A user agent nobody else sends
 * identifies this browser, and by extension its user, to every site they visit;
 * looking like the most common desktop browser is the private choice as well as
 * the compatible one.
 */
object UserAgents {

    /** Used only when the default string has no version to borrow. */
    private const val FALLBACK_VERSION = "120.0.0.0"

    fun desktop(default: String): String {
        val version = Regex("""Chrome/([0-9.]+)""").find(default)?.groupValues?.get(1)
            ?.takeIf { it.isNotEmpty() } ?: FALLBACK_VERSION
        return "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) " +
            "Chrome/$version Safari/537.36"
    }
}

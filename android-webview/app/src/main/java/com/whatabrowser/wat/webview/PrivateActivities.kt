package com.whatabrowser.wat.webview

/**
 * The private windows.
 *
 * Two classes rather than one with an extra, because the process a window runs
 * in is fixed in the manifest and the process is what gives each mode its own
 * cookie jar — and, for the lion, its own proxy. A window cannot change its mind
 * about which of those it is, and this is the shape that says so.
 */
class CatBrowserActivity : BrowserActivity() {
    override val privacy = PrivacyMode.CAT
}

class LionBrowserActivity : BrowserActivity() {
    override val privacy = PrivacyMode.LION
}

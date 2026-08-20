package com.whatabrowser.wat.webview

import android.content.Context
import android.webkit.CookieManager
import android.webkit.WebStorage
import java.io.File

/**
 * Throwing a private session away.
 *
 * Two halves, because either one alone leaves something behind. Clearing cookies
 * and site storage empties what the WebView holds in memory and in its own
 * databases; deleting the data directory removes the files underneath, including
 * the caches and the parts of the profile no API clears.
 *
 * The directory is only ever deleted at the start of a private process, before a
 * WebView exists — deleting files out from under a running WebView corrupts the
 * profile rather than clearing it, and a window that the system kills never runs
 * its own cleanup anyway.
 */
object PrivateStorage {

    /**
     * Deletes what a previous private session of this mode left on disk.
     *
     * Guarded twice over: the mode has to be a private one, and the directory
     * name has to be one of exactly two. A path built from a variable and then
     * deleted recursively is worth being paranoid about.
     */
    fun wipe(context: Context, mode: PrivacyMode) {
        if (!mode.isPrivate) return
        val name = "app_webview_${mode.storageSuffix}"
        if (name != "app_webview_cat" && name != "app_webview_lion") return

        val root = File(context.applicationInfo.dataDir)
        val directory = File(root, name)
        if (!directory.isDirectory) return
        runCatching { directory.deleteRecursively() }
    }

    /** What can be cleared while the window is still open, on its way out. */
    fun clearSession(mode: PrivacyMode) {
        if (!mode.isPrivate) return
        runCatching {
            val cookies = CookieManager.getInstance()
            cookies.removeAllCookies(null)
            cookies.flush()
            WebStorage.getInstance().deleteAllData()
        }
    }
}

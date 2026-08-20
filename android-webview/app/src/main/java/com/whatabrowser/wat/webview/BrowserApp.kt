package com.whatabrowser.wat.webview

import android.app.Application
import android.os.Build
import android.webkit.WebView
import androidx.annotation.RequiresApi

/**
 * Which storage jar this process gets.
 *
 * This is the whole mechanism behind private browsing here. Android's WebView
 * keeps one cookie and storage jar per *process*, and `setDataDirectorySuffix`
 * chooses which one — so a private window is not a flag on a tab, it is a second
 * process with a directory of its own that gets deleted. A private tab sharing
 * the ordinary process would share its cookies, and be private in name only.
 *
 * The suffix has to be set before any WebView is touched, which is why it happens
 * here and not in the activity.
 */
class BrowserApp : Application() {

    override fun onCreate() {
        super.onCreate()
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) return

        // Every process runs this, so the process name is what decides. The
        // private processes are declared in the manifest as `:cat` and `:lion`.
        val process = processName()
        val mode = when {
            process.endsWith(":cat") -> PrivacyMode.CAT
            process.endsWith(":lion") -> PrivacyMode.LION
            else -> PrivacyMode.NORMAL
        }
        WebView.setDataDirectorySuffix(mode.storageSuffix)

        // Whatever the last private window left behind, before anything can open
        // it again. Done on the way in rather than on the way out because a
        // window that is killed — by the system, or by the reader swiping it
        // away — never gets to run its own cleanup.
        PrivateStorage.wipe(this, mode)
    }

    // Only ever reached from behind the API 28 check above; the annotation is
    // what lets lint see that.
    @RequiresApi(Build.VERSION_CODES.P)
    private fun processName(): String = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        getProcessName()
    } else {
        @Suppress("DEPRECATION")
        Application.getProcessName()
    }
}

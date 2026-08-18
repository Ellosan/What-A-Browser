package com.whatabrowser.wat.webview

import android.app.Application
import android.os.Build
import android.webkit.WebView

class BrowserApp : Application() {
    override fun onCreate() {
        super.onCreate()
        // Android refuses to run more than one WebView in a process unless each
        // gets its own data directory. There is only one here, but naming it
        // makes a second process — which the system may start for a service —
        // fail loudly rather than at a random later moment.
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            WebView.setDataDirectorySuffix("wat")
        }
    }
}

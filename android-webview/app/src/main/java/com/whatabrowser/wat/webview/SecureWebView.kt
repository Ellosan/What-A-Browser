package com.whatabrowser.wat.webview

import android.annotation.SuppressLint
import android.webkit.CookieManager
import android.webkit.WebSettings
import android.webkit.WebView
import androidx.webkit.WebSettingsCompat
import androidx.webkit.WebViewFeature

/**
 * The WebView configuration, in one place, with reasons.
 *
 * A `WebView` out of the box is set up to host an app's own trusted content. A
 * browser points it at the open web, which inverts nearly every default: the
 * content is now hostile, and the things that make embedding convenient are the
 * things that get people owned. Each line below is switched off because of a
 * specific way it is abused, and the reason is written down so nobody switches
 * it back on to fix a bug without knowing the cost.
 */
object SecureWebView {

    @SuppressLint("SetJavaScriptEnabled")
    fun configure(webView: WebView, debuggable: Boolean) {
        val settings = webView.settings

        // A browser without JavaScript is not a browser. This is the one
        // dangerous default that has to stay on, which is why everything that
        // limits its reach below matters so much.
        settings.javaScriptEnabled = true

        // --- what a page must never be able to touch ---------------------

        // `file://` access is how a web page reads the app's private storage and
        // the user's documents. There is no local content in this browser, so
        // there is nothing to lose by refusing outright.
        settings.allowFileAccess = false
        settings.allowContentAccess = false

        // These two are the classic same-origin escape: they let a `file://`
        // document read every other file and issue cross-origin requests as if
        // it were any site. Both default to true on old platforms.
        @Suppress("DEPRECATION")
        settings.allowFileAccessFromFileURLs = false
        @Suppress("DEPRECATION")
        settings.allowUniversalAccessFromFileURLs = false

        // An HTTPS page pulling scripts over HTTP is an HTTPS page an attacker on
        // the network controls. `COMPATIBILITY_MODE` — the default — permits
        // exactly that for images and stylesheets.
        settings.mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW

        // Popups that open without a gesture, driven by the page. The pair
        // matters: multiple windows are supported, so `target="_blank"` and a
        // tapped `window.open` become tabs, but script cannot open one on its
        // own — and `onCreateWindow` refuses anything without a user gesture as
        // well, because this is the setting worth checking twice.
        settings.javaScriptCanOpenWindowsAutomatically = false
        settings.setSupportMultipleWindows(true)

        // Location is a permission the browser has not asked for and does not
        // want; the WebView would otherwise prompt on the page's behalf.
        settings.setGeolocationEnabled(false)

        // Deprecated storage APIs that are still reachable and still bugs.
        @Suppress("DEPRECATION")
        settings.databaseEnabled = false
        settings.saveFormData = false

        // --- what the browser does want -----------------------------------

        settings.domStorageEnabled = true
        settings.loadsImagesAutomatically = true
        settings.useWideViewPort = true
        settings.loadWithOverviewMode = true
        settings.setSupportZoom(true)
        settings.builtInZoomControls = true
        settings.displayZoomControls = false

        // Google's malware and phishing list. Backported by androidx.webkit to
        // devices whose framework does not have it.
        if (WebViewFeature.isFeatureSupported(WebViewFeature.SAFE_BROWSING_ENABLE)) {
            WebSettingsCompat.setSafeBrowsingEnabled(settings, true)
        }

        // Ask sites not to profile, for whatever it is worth, and switch off the
        // Topics/attribution surface where the platform lets us.
        if (WebViewFeature.isFeatureSupported(WebViewFeature.ATTRIBUTION_REGISTRATION_BEHAVIOR)) {
            WebSettingsCompat.setAttributionRegistrationBehavior(
                settings,
                WebSettingsCompat.ATTRIBUTION_BEHAVIOR_DISABLED,
            )
        }

        // Third-party cookies are the tracking mechanism and a CSRF ingredient.
        // First-party ones stay: sites stop working without them.
        val cookies = CookieManager.getInstance()
        cookies.setAcceptCookie(true)
        cookies.setAcceptThirdPartyCookies(webView, false)

        // Remote debugging is an open door to every page in the browser for
        // anything that can reach the device over ADB. Debug builds only.
        WebView.setWebContentsDebuggingEnabled(debuggable)

        // Deliberately absent: `addJavascriptInterface`. There is no bridge from
        // web content into this app at all. It is the single most reliable way to
        // turn a WebView into remote code execution, and a browser has no reason
        // to expose one.
    }
}

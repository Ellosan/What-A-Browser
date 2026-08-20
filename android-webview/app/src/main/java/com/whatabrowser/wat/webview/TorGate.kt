package com.whatabrowser.wat.webview

import android.content.Context
import android.content.Intent
import android.os.Handler
import android.os.Looper
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.webkit.ProxyConfig
import androidx.webkit.ProxyController
import androidx.webkit.WebViewFeature
import java.util.concurrent.Executor

/**
 * Routing a window through Tor, and refusing to open it if that has not worked.
 *
 * **What this is.** Every request from the lion window goes through Orbot's HTTP
 * proxy, so the sites visited see a Tor exit node rather than the phone's
 * address, and the network between the phone and Tor sees only a connection to
 * Tor.
 *
 * **What this is not.** It is not the Tor Browser. Tor Browser's real work is
 * making every user look identical — fonts, screen size, timing, canvas, the lot
 * — and none of that is possible in a system WebView. A site can still tell one
 * visitor from another. This hides *where you are connecting from*, not *who is
 * connecting*. The gate says so, once, before the first page.
 *
 * **Why it fails closed.** The proxy override carries no direct fallback, so if
 * Orbot stops, requests fail rather than quietly going out over the ordinary
 * network. And the window does not open at all until `check.torproject.org` has
 * confirmed, through this same proxied stack, that Tor is what it sees.
 *
 * SOCKS is not an option: `ProxyController` speaks HTTP proxies only, which is
 * why this needs Orbot's HTTP port rather than its SOCKS one.
 */
object TorGate {

    const val ORBOT_PACKAGE = "org.torproject.android"

    /** Orbot's HTTP proxy. Its SOCKS port cannot be used by a WebView. */
    const val PROXY = "127.0.0.1:8118"

    private const val CHECK_URL = "https://check.torproject.org/api/ip"

    private const val CHECK_TIMEOUT_MS = 45_000L

    private val direct = Executor { it.run() }

    /** Whether this device's WebView can be pointed at a proxy at all. */
    fun isSupported(): Boolean = WebViewFeature.isFeatureSupported(WebViewFeature.PROXY_OVERRIDE)

    /**
     * Sends everything in this process through the proxy.
     *
     * Process-wide, which is the second reason a lion window is a process of its
     * own: the ordinary window must not start using Tor because a private one was
     * opened, and it must not stop using it because the private one closed.
     */
    fun route(onApplied: () -> Unit) {
        if (!isSupported()) {
            onApplied()
            return
        }
        val config = ProxyConfig.Builder()
            .addProxyRule(PROXY)
            // Deliberately no `addDirect()`: with no fallback, a proxy that is
            // not there means the page fails to load rather than loading over
            // the ordinary network.
            .build()
        ProxyController.getInstance().setProxyOverride(config, direct, onApplied)
    }

    fun clear(onCleared: () -> Unit = {}) {
        if (!isSupported()) {
            onCleared()
            return
        }
        ProxyController.getInstance().clearProxyOverride(direct, onCleared)
    }

    fun orbotInstalled(context: Context): Boolean = try {
        context.packageManager.getPackageInfo(ORBOT_PACKAGE, 0)
        true
    } catch (_: Exception) {
        false
    }

    /** Asks Orbot to start. It shows its own interface; there is no silent start. */
    fun startOrbot(context: Context): Boolean = try {
        val intent = context.packageManager.getLaunchIntentForPackage(ORBOT_PACKAGE)
            ?: return false
        intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        context.startActivity(intent)
        true
    } catch (_: Exception) {
        false
    }

    /**
     * Asks Tor's own check service whether this is Tor, through the WebView that
     * will do the browsing.
     *
     * Through the WebView on purpose: a check made with the app's own HTTP client
     * would answer for the app's connection, not for the proxied one, and could
     * say yes while every page still went out in the clear.
     *
     * The answer is read with `evaluateJavascript`, which the app calls into the
     * page — not with a JavaScript interface, which is the page calling into the
     * app and is the thing this browser refuses to have anywhere.
     */
    fun verify(webView: WebView, onResult: (Boolean) -> Unit) {
        val handler = Handler(Looper.getMainLooper())
        var settled = false

        val finish = { answer: Boolean ->
            if (!settled) {
                settled = true
                handler.removeCallbacksAndMessages(null)
                onResult(answer)
            }
        }

        webView.webViewClient = object : WebViewClient() {
            override fun onPageFinished(view: WebView, url: String) {
                view.evaluateJavascript("document.body ? document.body.innerText : null") { value ->
                    finish(TorCheck.isTor(value))
                }
            }

            override fun onReceivedError(
                view: WebView,
                request: android.webkit.WebResourceRequest,
                error: android.webkit.WebResourceError,
            ) {
                // Only the check page's own failure counts; a favicon that did
                // not load is not an answer.
                if (request.isForMainFrame) finish(false)
            }
        }

        // A proxy that accepts the connection and then never answers would
        // otherwise leave the window waiting forever on its gate.
        handler.postDelayed({ finish(false) }, CHECK_TIMEOUT_MS)
        webView.loadUrl(CHECK_URL)
    }
}

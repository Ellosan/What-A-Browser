package com.whatabrowser.wat.webview

import android.content.Context
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
 * **What this is.** Tor runs inside the app — see [TorEngine] — and every request
 * from the lion window goes through it, so the sites visited see a Tor exit node
 * rather than the phone's address, and the network between the phone and Tor sees
 * only a connection to Tor. Nothing else needs installing.
 *
 * **What this is not.** It is not the Tor Browser. Tor Browser's real work is
 * making every user look identical — fonts, screen size, timing, canvas, the lot
 * — and none of that is possible in a system WebView. A site can still tell one
 * visitor from another. This hides *where you are connecting from*, not *who is
 * connecting*. The gate says so, once, before the first page.
 *
 * **Why it fails closed.** The proxy override carries no direct fallback, so if
 * tor stops, requests fail rather than quietly going out over the ordinary
 * network. And the window does not open at all until `check.torproject.org` has
 * confirmed, through this same proxied stack, that Tor is what it sees.
 */
object TorGate {

    private const val CHECK_URL = "https://check.torproject.org/api/ip"

    private const val CHECK_TIMEOUT_MS = 60_000L

    private val direct = Executor { it.run() }

    /** Whether this device's WebView can be pointed at a proxy at all. */
    fun isSupported(): Boolean = WebViewFeature.isFeatureSupported(WebViewFeature.PROXY_OVERRIDE)

    /** Starts tor and the local bridge in front of it. */
    fun start(context: Context, onState: (TorEngine.State) -> Unit) =
        TorEngine.start(context, onState)

    fun stop() = TorEngine.stop()

    /**
     * Sends everything in this process through [proxy].
     *
     * Process-wide, which is the second reason a lion window is a process of its
     * own: the ordinary window must not start using Tor because a private one was
     * opened, and it must not stop using it because the private one closed.
     */
    fun route(proxy: String, onApplied: () -> Unit) {
        if (!isSupported()) {
            onApplied()
            return
        }
        val config = ProxyConfig.Builder()
            .addProxyRule(proxy)
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

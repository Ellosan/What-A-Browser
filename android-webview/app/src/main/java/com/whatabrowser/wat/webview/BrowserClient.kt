package com.whatabrowser.wat.webview

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.net.Uri
import android.net.http.SslError
import android.webkit.SslErrorHandler
import android.webkit.WebResourceRequest
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.Toast

/**
 * Navigation policy: what the browser will load, and what it refuses.
 *
 * @param onNavigation called with the URL and whether the load has finished, so
 *   the chrome can follow along without reaching into the WebView.
 */
class BrowserClient(
    private val context: Context,
    private val onNavigation: (url: String, loading: Boolean) -> Unit,
) : WebViewClient() {

    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
        val url = request.url.toString()
        return when (UrlResolver.decide(url)) {
            UrlResolver.Decision.RENDER -> false // let the WebView have it
            UrlResolver.Decision.ASK_SYSTEM -> {
                handOff(request.url)
                true
            }
            UrlResolver.Decision.REFUSE -> {
                // Silence would look like a broken link; saying so makes it clear
                // the browser decided, not the site.
                Toast.makeText(
                    context,
                    context.getString(R.string.blocked_scheme, request.url.scheme ?: "?"),
                    Toast.LENGTH_SHORT,
                ).show()
                true
            }
        }
    }

    /**
     * A certificate error is the end of the navigation.
     *
     * `handler.proceed()` is the single most common critical vulnerability in
     * Android apps that embed a WebView: it turns every HTTPS page into one an
     * attacker on the network can rewrite. There is no user-visible override
     * here on purpose — a "continue anyway" button is the same hole with a
     * consent dialog in front of it, and people click through those.
     */
    override fun onReceivedSslError(view: WebView, handler: SslErrorHandler, error: SslError) {
        handler.cancel()
        val detail = when (error.primaryError) {
            SslError.SSL_EXPIRED -> R.string.ssl_expired
            SslError.SSL_IDMISMATCH -> R.string.ssl_mismatch
            SslError.SSL_UNTRUSTED -> R.string.ssl_untrusted
            SslError.SSL_DATE_INVALID -> R.string.ssl_date
            else -> R.string.ssl_generic
        }
        Toast.makeText(context, context.getString(detail), Toast.LENGTH_LONG).show()
        onNavigation(view.url.orEmpty(), false)
    }

    override fun onPageStarted(view: WebView, url: String, favicon: Bitmap?) {
        onNavigation(url, true)
    }

    override fun onPageFinished(view: WebView, url: String) {
        onNavigation(url, false)
    }

    /** Opens a `mailto:`, `tel:` or `sms:` link with whatever handles it. */
    private fun handOff(uri: Uri) {
        val intent = Intent(Intent.ACTION_VIEW, uri).apply {
            // `FLAG_ACTIVITY_NEW_TASK` because this leaves the browser's task,
            // and the selector keeps a malicious page from aiming the intent at
            // a specific component of its choosing.
            addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            selector = null
        }
        try {
            context.startActivity(intent)
        } catch (_: ActivityNotFoundException) {
            Toast.makeText(
                context,
                context.getString(R.string.no_handler, uri.scheme ?: "?"),
                Toast.LENGTH_SHORT,
            ).show()
        }
    }
}

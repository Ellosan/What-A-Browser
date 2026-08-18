package com.whatabrowser.wat.webview

import android.annotation.SuppressLint
import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.view.View
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.webkit.WebChromeClient
import android.webkit.WebView
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ProgressBar
import android.widget.TextView

/**
 * The browser: a system WebView with WAT's interface around it.
 *
 * A plain [Activity], not `AppCompatActivity`, and views rather than Compose.
 * Both are the same decision — every library linked here is initialised before
 * the first frame, and this build exists because launching quickly matters more
 * than the convenience of the alternatives.
 */
class BrowserActivity : Activity() {

    private lateinit var web: WebView
    private lateinit var address: EditText
    private lateinit var lock: TextView
    private lateinit var progress: ProgressBar

    private val home = "https://duckduckgo.com/"
    private val search = "https://duckduckgo.com/?q={}"

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_browser)

        web = findViewById(R.id.web)
        address = findViewById(R.id.address)
        lock = findViewById(R.id.lock)
        progress = findViewById(R.id.progress)

        applyGlass()

        SecureWebView.configure(web, debuggable = BuildConfig.DEBUG)
        web.webViewClient = BrowserClient(this) { url, loading -> onNavigation(url, loading) }
        web.webChromeClient = object : WebChromeClient() {
            override fun onProgressChanged(view: WebView, newProgress: Int) {
                progress.visibility = if (newProgress in 1..99) View.VISIBLE else View.GONE
            }
        }

        findViewById<View>(R.id.back).setOnClickListener { if (web.canGoBack()) web.goBack() }
        findViewById<View>(R.id.forward).setOnClickListener { if (web.canGoForward()) web.goForward() }
        findViewById<View>(R.id.reload).setOnClickListener { web.reload() }
        findViewById<View>(R.id.home).setOnClickListener { load(home) }

        address.setOnEditorActionListener { _, actionId, _ ->
            if (actionId == EditorInfo.IME_ACTION_GO) {
                load(UrlResolver.resolve(address.text.toString(), search))
                address.clearFocus()
                hideKeyboard()
                true
            } else {
                false
            }
        }

        // A link that started the app takes precedence over the home page.
        val opened = intent?.takeIf { it.action == Intent.ACTION_VIEW }?.dataString
        if (savedInstanceState == null) {
            load(opened ?: home)
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        if (intent.action == Intent.ACTION_VIEW) {
            intent.dataString?.let(::load)
        }
    }

    /** Restores the page and its history rather than reloading from the network. */
    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        web.saveState(outState)
    }

    override fun onRestoreInstanceState(savedInstanceState: Bundle) {
        super.onRestoreInstanceState(savedInstanceState)
        web.restoreState(savedInstanceState)
    }

    @Suppress("DEPRECATION")
    override fun onBackPressed() {
        // Back means back through the page's history first, and only leaves the
        // browser when there is nowhere left to go — the same rule the other
        // Android app follows.
        if (web.canGoBack()) web.goBack() else super.onBackPressed()
    }

    override fun onPause() {
        super.onPause()
        web.onPause()
    }

    override fun onResume() {
        super.onResume()
        web.onResume()
    }

    override fun onDestroy() {
        // A WebView outlives its activity if it is left attached, and takes the
        // whole view tree with it.
        (web.parent as? android.view.ViewGroup)?.removeView(web)
        web.destroy()
        super.onDestroy()
    }

    private fun load(url: String) {
        if (url.isBlank()) return
        when (UrlResolver.decide(url)) {
            UrlResolver.Decision.RENDER -> web.loadUrl(url)
            // Anything else typed or handed in is refused rather than passed to
            // `loadUrl`, which would happily run a `javascript:` URL in whatever
            // page is currently open.
            else -> address.setText(web.url.orEmpty())
        }
    }

    private fun onNavigation(url: String, loading: Boolean) {
        if (!address.hasFocus()) address.setText(url)
        lock.visibility = if (url.startsWith("https://")) View.VISIBLE else View.GONE
        progress.visibility = if (loading) View.VISIBLE else View.GONE
    }

    private fun applyGlass() {
        findViewById<LinearLayout>(R.id.top_bar).background =
            GlassSurface.panel(this, GlassGeometry.radiusLarge)
        findViewById<LinearLayout>(R.id.bottom_bar).background =
            GlassSurface.panel(this, GlassGeometry.radiusLarge)
        address.background = GlassSurface.pill(this)
    }

    private fun hideKeyboard() {
        (getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager)
            .hideSoftInputFromWindow(address.windowToken, 0)
    }
}

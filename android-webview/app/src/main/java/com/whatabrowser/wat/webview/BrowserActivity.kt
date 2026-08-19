package com.whatabrowser.wat.webview

import android.app.Activity
import android.app.AlertDialog
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.text.TextUtils
import android.view.KeyEvent
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebView
import android.widget.AutoCompleteTextView
import android.widget.Button
import android.widget.EditText
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.PopupMenu
import android.widget.ProgressBar
import android.widget.TextView
import android.widget.Toast

/**
 * The browser: system WebViews with WAT's interface around them.
 *
 * A plain [Activity], not `AppCompatActivity`, and views rather than Compose.
 * Both are the same decision — every library linked here is initialised before
 * the first frame, and this build exists because launching quickly matters more
 * than the convenience of the alternatives.
 *
 * The activity is the interface and nothing else. [Tabs] owns the pages,
 * [BrowserStore] owns history and bookmarks, [SecureWebView] owns what a page is
 * allowed to do, and the parts worth testing — addresses, tab order, download
 * names — are in classes with no Android in them at all.
 */
class BrowserActivity : Activity(), Tabs.Listener {

    private lateinit var tabs: Tabs
    private lateinit var store: BrowserStore
    private lateinit var settings: Settings

    private lateinit var address: AutoCompleteTextView
    private lateinit var lock: TextView
    private lateinit var progress: ProgressBar
    private lateinit var tabCount: TextView
    private lateinit var bookmark: Button
    private lateinit var findBar: LinearLayout
    private lateinit var findInput: EditText
    private lateinit var findMatches: TextView
    private lateinit var fullscreen: FrameLayout

    /** Held between showing the file chooser and hearing back from it. */
    private var pendingFiles: ValueCallback<Array<Uri>>? = null

    private var fullscreenCallback: WebChromeClient.CustomViewCallback? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_browser)

        address = findViewById(R.id.address)
        lock = findViewById(R.id.lock)
        progress = findViewById(R.id.progress)
        tabCount = findViewById(R.id.tab_count)
        bookmark = findViewById(R.id.bookmark)
        findBar = findViewById(R.id.find_bar)
        findInput = findViewById(R.id.find_input)
        findMatches = findViewById(R.id.find_matches)
        fullscreen = findViewById(R.id.fullscreen)

        applyGlass()

        settings = Settings(this)
        store = BrowserStore(this)
        tabs = Tabs(this, findViewById(R.id.pages), settings, this)

        wireChrome()

        // A link that started the app takes precedence over the home page.
        val opened = intent?.takeIf { it.action == Intent.ACTION_VIEW }?.dataString
        // A restored session already contains whatever link started the app the
        // first time; opening it again here would add a tab on every restore.
        val restored = savedInstanceState?.let(tabs::restoreInstanceState) ?: false
        if (!restored) tabs.open(opened ?: settings.homePage)
    }

    private fun wireChrome() {
        findViewById<View>(R.id.back).setOnClickListener { tabs.goBack() }
        findViewById<View>(R.id.forward).setOnClickListener { tabs.goForward() }
        findViewById<View>(R.id.reload).setOnClickListener { tabs.reload() }
        findViewById<View>(R.id.home).setOnClickListener { load(settings.homePage) }
        bookmark.setOnClickListener { toggleBookmark() }
        tabCount.setOnClickListener { showTabs() }
        findViewById<View>(R.id.menu).setOnClickListener(::showMenu)

        address.setAdapter(SuggestionAdapter(this, store))
        address.setOnItemClickListener { parent, _, position, _ ->
            (parent.getItemAtPosition(position) as? BrowserStore.Entry)?.let {
                address.clearFocus()
                hideKeyboard(address)
                load(it.url)
            }
        }
        address.setOnEditorActionListener { _, actionId, _ ->
            if (actionId == EditorInfo.IME_ACTION_GO) {
                load(UrlResolver.resolve(address.text.toString(), settings.searchTemplate))
                address.clearFocus()
                hideKeyboard(address)
                true
            } else {
                false
            }
        }

        findInput.setOnEditorActionListener { _, actionId, event ->
            val pressed = actionId == EditorInfo.IME_ACTION_SEARCH ||
                event?.keyCode == KeyEvent.KEYCODE_ENTER
            if (pressed) tabs.find(findInput.text.toString())
            pressed
        }
        findViewById<View>(R.id.find_next).setOnClickListener { tabs.findNext(true) }
        findViewById<View>(R.id.find_previous).setOnClickListener { tabs.findNext(false) }
        findViewById<View>(R.id.find_close).setOnClickListener { hideFind() }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        // A link from another app opens in a tab of its own rather than replacing
        // whatever was being read.
        if (intent.action == Intent.ACTION_VIEW) {
            intent.dataString?.let { tabs.open(it) }
        }
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        tabs.saveInstanceState(outState)
    }

    @Deprecated("Deprecated in Java")
    @Suppress("DEPRECATION")
    override fun onBackPressed() {
        // Back unwinds what is on top of the page first — fullscreen video, then
        // the find bar — then the page's own history, then the tab, and only
        // leaves the browser when there is nothing left to go back to.
        when {
            fullscreenCallback != null -> onFullscreen(null, null)
            findBar.visibility == View.VISIBLE -> hideFind()
            tabs.goBack() -> Unit
            tabs.count > 1 -> closeTab(tabs.currentTab()?.id ?: return)
            else -> super.onBackPressed()
        }
    }

    override fun onPause() {
        super.onPause()
        tabs.onPause()
    }

    override fun onResume() {
        super.onResume()
        tabs.onResume()
    }

    override fun onDestroy() {
        // A WebView outlives its activity if it is left attached, and takes the
        // whole view tree with it.
        tabs.destroy()
        super.onDestroy()
    }

    // --- loading -----------------------------------------------------------

    private fun load(url: String) {
        if (url.isBlank()) return
        when (UrlResolver.decide(url)) {
            UrlResolver.Decision.RENDER -> tabs.load(url)
            // Anything else typed or handed in is refused rather than passed to
            // `loadUrl`, which would happily run a `javascript:` URL in whatever
            // page is currently open.
            else -> address.setText(tabs.currentTab()?.url.orEmpty())
        }
    }

    // --- what the tabs report back ------------------------------------------

    override fun onPageChanged(url: String, title: String, loading: Boolean) {
        // `setText(text, false)` rather than `setText(text)`: the address bar is
        // an autocomplete field, and filling it in with the page's own URL must
        // not open the suggestion list.
        if (!address.hasFocus()) address.setText(url, false)
        lock.visibility = if (url.startsWith("https://")) View.VISIBLE else View.GONE
        if (!loading) progress.visibility = View.GONE
        refreshBookmarkButton(url)
    }

    override fun onProgress(percent: Int) {
        progress.progress = percent
        progress.visibility = if (percent in 1..99) View.VISIBLE else View.GONE
    }

    override fun onTabsChanged() {
        tabCount.text = tabs.count.toString()
        tabCount.contentDescription = getString(R.string.tabs)
    }

    override fun onVisit(url: String, title: String) {
        if (settings.keepHistory) store.recordVisit(url, title)
    }

    override fun onTitle(url: String, title: String) {
        if (settings.keepHistory) store.recordTitle(url, title)
    }

    override fun onDownloadRequested(
        url: String,
        userAgent: String?,
        contentDisposition: String?,
        mimeType: String?,
    ) {
        DownloadQueue.start(this, url, userAgent, contentDisposition, mimeType)
    }

    /**
     * A page asked for a file.
     *
     * The chooser the platform builds asks the document picker, so this needs no
     * storage permission and the page never learns anything about the phone
     * beyond the file that was handed to it. The callback must be answered on
     * every path — a chooser that is cancelled without an answer leaves the file
     * input on the page dead until it is reloaded.
     */
    override fun onFileChooser(
        callback: ValueCallback<Array<Uri>>,
        params: WebChromeClient.FileChooserParams,
    ): Boolean {
        pendingFiles?.onReceiveValue(null)
        pendingFiles = callback
        return try {
            startActivityForResult(params.createIntent(), REQUEST_FILES)
            true
        } catch (_: Exception) {
            pendingFiles = null
            callback.onReceiveValue(null)
            false
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        if (requestCode != REQUEST_FILES) return
        val callback = pendingFiles ?: return
        pendingFiles = null
        callback.onReceiveValue(WebChromeClient.FileChooserParams.parseResult(resultCode, data))
    }

    override fun onFindResult(activeMatch: Int, matches: Int) {
        findMatches.text = if (matches == 0) {
            getString(R.string.find_none)
        } else {
            getString(R.string.find_matches, activeMatch + 1, matches)
        }
    }

    /** Fullscreen video: the page's view over everything, bars and all. */
    override fun onFullscreen(view: View?, exit: WebChromeClient.CustomViewCallback?) {
        if (view == null) {
            fullscreen.removeAllViews()
            fullscreen.visibility = View.GONE
            fullscreenCallback?.onCustomViewHidden()
            fullscreenCallback = null
            return
        }
        fullscreenCallback = exit
        fullscreen.addView(
            view,
            FrameLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
            ),
        )
        fullscreen.visibility = View.VISIBLE
    }

    override fun onTabLimitReached() {
        Toast.makeText(this, R.string.tab_limit, Toast.LENGTH_SHORT).show()
    }

    /**
     * The menu on a long press: what to do with the link or image underneath.
     *
     * Only for targets the browser would load anyway. A menu offering to open a
     * `javascript:` link in a new tab is a menu that talks people into running
     * one.
     */
    override fun onLongPress(type: Int, target: String?): Boolean {
        val url = target ?: return false
        val isLink = type == WebView.HitTestResult.SRC_ANCHOR_TYPE ||
            type == WebView.HitTestResult.SRC_IMAGE_ANCHOR_TYPE
        val isImage = type == WebView.HitTestResult.IMAGE_TYPE ||
            type == WebView.HitTestResult.SRC_IMAGE_ANCHOR_TYPE
        if (!isLink && !isImage) return false
        if (UrlResolver.decide(url) != UrlResolver.Decision.RENDER) return false

        val labels = listOf(
            getString(R.string.open_in_new_tab),
            getString(R.string.copy_link),
            getString(R.string.share),
            getString(R.string.download_link),
        )
        AlertDialog.Builder(this)
            .setTitle(url)
            .setItems(labels.toTypedArray()) { _, which ->
                when (which) {
                    0 -> tabs.open(url, foreground = false)
                    1 -> copy(url)
                    2 -> share(url, url)
                    3 -> DownloadQueue.start(this, url, null, null, null)
                }
            }
            .show()
        return true
    }

    // --- the panels ---------------------------------------------------------

    private fun showMenu(anchor: View) {
        val menu = PopupMenu(this, anchor)
        val bookmarked = tabs.currentTab()?.url?.let(store::isBookmarked) ?: false
        menu.menu.add(0, MENU_NEW_TAB, 0, R.string.new_tab)
        menu.menu.add(0, MENU_BOOKMARK, 0, if (bookmarked) R.string.remove_bookmark else R.string.add_bookmark)
        menu.menu.add(0, MENU_BOOKMARKS, 0, R.string.bookmarks)
        menu.menu.add(0, MENU_HISTORY, 0, R.string.history)
        menu.menu.add(0, MENU_FIND, 0, R.string.find_in_page)
        menu.menu.add(0, MENU_SHARE, 0, R.string.share_page)
        menu.menu.add(0, MENU_DOWNLOADS, 0, R.string.downloads)
        menu.menu.add(0, MENU_DESKTOP, 0, R.string.desktop_site).apply {
            isCheckable = true
            isChecked = tabs.isDesktopSite
        }
        menu.menu.add(0, MENU_SETTINGS, 0, R.string.settings)

        menu.setOnMenuItemClickListener { item ->
            when (item.itemId) {
                MENU_NEW_TAB -> tabs.open(settings.homePage)
                MENU_BOOKMARK -> toggleBookmark()
                MENU_BOOKMARKS -> showBookmarks()
                MENU_HISTORY -> showHistory()
                MENU_FIND -> showFind()
                MENU_SHARE -> sharePage()
                MENU_DOWNLOADS -> DownloadQueue.showAll(this)
                MENU_DESKTOP -> tabs.setDesktopSite(!tabs.isDesktopSite)
                MENU_SETTINGS -> showSettings()
                else -> return@setOnMenuItemClickListener false
            }
            true
        }
        menu.show()
    }

    private fun showTabs() {
        val current = tabs.currentTab()?.id
        val rows = tabs.all().map { tab ->
            Panels.Row(
                title = tab.label,
                subtitle = UrlResolver.hostOf(tab.url),
                current = tab.id == current,
                onRemove = { closeTab(tab.id) },
                onClick = { tabs.select(tab.id) },
            )
        }
        Panels.list(
            this,
            getString(R.string.tabs),
            rows,
            getString(R.string.no_tabs),
            getString(R.string.new_tab) to { tabs.open(settings.homePage) },
        ).show()
    }

    private fun showBookmarks() {
        val saved = store.bookmarks()
        val rows = saved.map { entry ->
            Panels.Row(
                title = entry.label,
                subtitle = entry.url,
                onRemove = { store.removeBookmark(entry.url) },
                onClick = { load(entry.url) },
            )
        }
        Panels.list(this, getString(R.string.bookmarks), rows, getString(R.string.no_bookmarks)).show()
    }

    private fun showHistory() {
        val visited = store.history()
        val rows = visited.map { entry ->
            Panels.Row(title = entry.label, subtitle = entry.url, onClick = { load(entry.url) })
        }
        Panels.list(
            this,
            getString(R.string.history),
            rows,
            getString(R.string.no_history),
            getString(R.string.clear_history) to { store.clearHistory() },
        ).show()
    }

    private fun showSettings() {
        SettingsPanel.show(this, settings, store) {
            // The home page or the engine may have moved; nothing on screen
            // depends on either until the next tap, so there is nothing to redraw.
        }.show()
    }

    // --- find in page --------------------------------------------------------

    private fun showFind() {
        findBar.visibility = View.VISIBLE
        findMatches.text = ""
        findInput.setText("")
        findInput.requestFocus()
        (getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager)
            .showSoftInput(findInput, InputMethodManager.SHOW_IMPLICIT)
    }

    private fun hideFind() {
        findBar.visibility = View.GONE
        tabs.clearFind()
        hideKeyboard(findInput)
    }

    // --- bookmarks, sharing, copying -----------------------------------------

    private fun toggleBookmark() {
        val tab = tabs.currentTab() ?: return
        if (UrlResolver.decide(tab.url) != UrlResolver.Decision.RENDER) return
        val nowBookmarked = store.toggleBookmark(tab.url, tab.title)
        refreshBookmarkButton(tab.url, nowBookmarked)
        Toast.makeText(
            this,
            if (nowBookmarked) R.string.bookmark_added else R.string.bookmark_removed,
            Toast.LENGTH_SHORT,
        ).show()
    }

    private fun refreshBookmarkButton(url: String, known: Boolean? = null) {
        if (known == null) {
            store.bookmarked(url) { bookmarked ->
                // The answer arrives later, by which time the reader may have
                // navigated again; a star for the page before this one would be
                // worse than a star that appears a moment late.
                if (tabs.currentTab()?.url == url) showBookmarked(bookmarked)
            }
            return
        }
        showBookmarked(known)
    }

    private fun showBookmarked(bookmarked: Boolean) {
        bookmark.text = getString(
            if (bookmarked) R.string.star_full_glyph else R.string.star_empty_glyph,
        )
        bookmark.contentDescription = getString(
            if (bookmarked) R.string.remove_bookmark else R.string.add_bookmark,
        )
    }

    private fun sharePage() {
        val tab = tabs.currentTab()
        if (tab == null || tab.url.isBlank()) {
            Toast.makeText(this, R.string.nothing_to_share, Toast.LENGTH_SHORT).show()
            return
        }
        share(tab.url, tab.label)
    }

    private fun share(url: String, subject: String) {
        val intent = Intent(Intent.ACTION_SEND).apply {
            type = "text/plain"
            putExtra(Intent.EXTRA_TEXT, url)
            putExtra(Intent.EXTRA_SUBJECT, subject)
        }
        startActivity(Intent.createChooser(intent, getString(R.string.share)))
    }

    private fun copy(text: String) {
        (getSystemService(CLIPBOARD_SERVICE) as ClipboardManager)
            .setPrimaryClip(ClipData.newPlainText(getString(R.string.app_name), text))
        Toast.makeText(this, R.string.copied, Toast.LENGTH_SHORT).show()
    }

    private fun closeTab(id: Int) {
        // An empty browser is not a state this app has; closing the last tab
        // leaves a fresh home page instead.
        if (!tabs.close(id)) tabs.open(settings.homePage)
    }

    // --- looks ---------------------------------------------------------------

    private fun applyGlass() {
        findViewById<LinearLayout>(R.id.top_bar).background =
            GlassSurface.panel(this, GlassGeometry.radiusLarge)
        findViewById<LinearLayout>(R.id.bottom_bar).background =
            GlassSurface.panel(this, GlassGeometry.radiusLarge)
        findBar.background = GlassSurface.panel(this, GlassGeometry.radiusMedium)
        address.background = GlassSurface.pill(this)
        tabCount.background = GlassSurface.pill(this)
        address.ellipsize = TextUtils.TruncateAt.END
    }

    private fun hideKeyboard(view: View) {
        (getSystemService(INPUT_METHOD_SERVICE) as InputMethodManager)
            .hideSoftInputFromWindow(view.windowToken, 0)
    }

    private companion object {
        const val REQUEST_FILES = 1

        const val MENU_NEW_TAB = 1
        const val MENU_BOOKMARK = 2
        const val MENU_BOOKMARKS = 3
        const val MENU_HISTORY = 4
        const val MENU_FIND = 5
        const val MENU_SHARE = 6
        const val MENU_DOWNLOADS = 7
        const val MENU_DESKTOP = 8
        const val MENU_SETTINGS = 9
    }
}

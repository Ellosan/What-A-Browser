package com.whatabrowser.wat.webview

import android.app.Activity
import android.app.AlertDialog
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.content.res.Configuration
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.os.Process
import android.text.TextUtils
import android.view.KeyEvent
import android.view.View
import android.view.WindowManager
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
open class BrowserActivity : Activity(), Tabs.Listener {

    /**
     * Which window this is. Fixed at compile time by the subclass, because the
     * process it runs in — and so the cookie jar it uses — is fixed in the
     * manifest. See [PrivacyMode].
     */
    protected open val privacy: PrivacyMode = PrivacyMode.NORMAL

    private lateinit var tabs: Tabs
    private lateinit var settings: Settings

    /**
     * History and bookmarks, in the ordinary window only.
     *
     * Null in a private window, and not merely unused: the database belongs to
     * the ordinary process, and a private window neither writes to it nor reads
     * it. Sharing one SQLite file across processes is unsafe in Android's own
     * documentation, and a private window that could read your bookmarks is not
     * the thing the name promises.
     */
    private var store: BrowserStore? = null

    private var backdrop: Backdrop? = null

    /**
     * The accelerometer, which moves the highlight on the glass.
     *
     * Registered only while the window is in front. A sensor left listening is a
     * battery complaint nobody can trace back to a highlight.
     */
    private var tilt: Tilt? = null

    private lateinit var address: AutoCompleteTextView
    private lateinit var lock: TextView
    private lateinit var progress: ProgressBar
    private lateinit var tabCount: TextView
    private lateinit var bookmark: Button
    private lateinit var findBar: GlassBar
    private lateinit var findInput: EditText
    private lateinit var findMatches: TextView
    private lateinit var fullscreen: FrameLayout
    private lateinit var modeBadge: TextView

    /** Held between showing the file chooser and hearing back from it. */
    private var pendingFiles: ValueCallback<Array<Uri>>? = null

    private var fullscreenCallback: WebChromeClient.CustomViewCallback? = null

    /** See [applyPrivacy]: set on the first page, not on the empty window. */
    private var screenshotsBlocked = false

    /**
     * Applies the theme setting.
     *
     * A plain `Activity` has no `AppCompatDelegate.setDefaultNightMode`, so the
     * night bit is set on the configuration this window is built from. It has to
     * happen here, before any resource is read, which is why choosing a theme in
     * settings restarts the window.
     */
    override fun attachBaseContext(base: Context) {
        val choice = Settings(base).theme
        if (choice == Settings.ThemeChoice.SYSTEM) {
            super.attachBaseContext(base)
            return
        }
        val configuration = Configuration(base.resources.configuration)
        configuration.uiMode = (configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK.inv()) or
            if (choice == Settings.ThemeChoice.DARK) {
                Configuration.UI_MODE_NIGHT_YES
            } else {
                Configuration.UI_MODE_NIGHT_NO
            }
        super.attachBaseContext(base.createConfigurationContext(configuration))
    }

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
        modeBadge = findViewById(R.id.mode_badge)

        applyGlass()
        applyPrivacy()

        settings = Settings(this)
        if (privacy.recordsHistory) store = BrowserStore(this)
        tabs = Tabs(this, findViewById(R.id.pages), settings, this)

        wireChrome()

        // A link that started the app takes precedence over the home page.
        val opened = intent?.takeIf { it.action == Intent.ACTION_VIEW }?.dataString
        // A restored session already contains whatever link started the app the
        // first time; opening it again here would add a tab on every restore.
        //
        // A private window never restores. Two reasons, and the second is the
        // serious one: a private session is meant to end when its window closes,
        // and in the Tor window a restored tab would start loading before the
        // gate below has confirmed anything — over the ordinary network.
        val restored = if (privacy.isPrivate) {
            false
        } else {
            savedInstanceState?.let(tabs::restoreInstanceState) ?: false
        }
        val first = opened ?: settings.homePage

        if (privacy.usesTor) {
            // Nothing loads until Tor has been confirmed, so the gate goes up
            // before the first tab rather than over it.
            openThroughTor(if (restored) null else first)
        } else if (!restored) {
            tabs.open(first)
            if (privacy.isPrivate) showPrivateNotice()
        }
    }

    /**
     * What a private window does differently before it shows anything.
     */
    private fun applyPrivacy() {
        if (!privacy.isPrivate) return

        // `FLAG_SECURE` is applied when the first page loads, not here. It keeps
        // a private window out of screenshots and out of the recents thumbnail,
        // which is right — but in 0.1.3 it also made the Tor window's own error
        // message impossible to screenshot, and an error nobody can get off the
        // phone is an error nobody can act on. Before a page has loaded there is
        // nothing private on screen to protect.
        modeBadge.visibility = View.VISIBLE
        modeBadge.text = getString(
            if (privacy.usesTor) R.string.lion_glyph else R.string.cat_glyph,
        )
        modeBadge.contentDescription = getString(
            if (privacy.usesTor) R.string.private_lion else R.string.private_cat,
        )
    }

    /**
     * Blocks screen capture from the moment a private window has a page in it.
     *
     * Deliberately not before: the gate and its failures are the one thing in a
     * private window worth being able to photograph, and they contain nothing
     * about what anyone was reading.
     */
    private fun blockScreenshotsOnce(url: String) {
        if (screenshotsBlocked || !privacy.isPrivate) return
        if (UrlResolver.decide(url) != UrlResolver.Decision.RENDER) return
        screenshotsBlocked = true
        window.setFlags(
            WindowManager.LayoutParams.FLAG_SECURE,
            WindowManager.LayoutParams.FLAG_SECURE,
        )
    }

    private fun showPrivateNotice() {
        AlertDialog.Builder(this)
            .setTitle(if (privacy.usesTor) R.string.private_lion else R.string.private_cat)
            .setMessage(R.string.private_notice)
            .setPositiveButton(android.R.string.ok, null)
            .show()
    }

    private fun wireChrome() {
        val back = findViewById<View>(R.id.back)
        val forward = findViewById<View>(R.id.forward)
        back.setOnClickListener { tabs.goBack() }
        forward.setOnClickListener { tabs.goForward() }

        // Holding back or forward offers the history rather than making eleven
        // taps out of going back to a search result.
        back.setOnLongClickListener { showHistoryStrip(backwards = true) }
        forward.setOnLongClickListener { showHistoryStrip(backwards = false) }
        findViewById<View>(R.id.reload).setOnClickListener { tabs.reload() }
        findViewById<View>(R.id.home).setOnClickListener { load(settings.homePage) }
        bookmark.setOnClickListener { toggleBookmark() }
        tabCount.setOnClickListener { showTabs() }
        findViewById<View>(R.id.menu).setOnClickListener(::showMenu)

        // A private window suggests nothing, because it can see nothing: there
        // is no store behind it to suggest from. Nor does an ordinary one, if
        // the reader would rather it did not.
        store?.takeIf { privacy.suggestsFromHistory && settings.suggestFromHistory }
            ?.let { address.setAdapter(SuggestionAdapter(this, it)) }
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

        val bars = listOf<GlassBar>(
            findViewById(R.id.top_bar),
            findViewById(R.id.bottom_bar),
            findBar,
        )
        backdrop = Backdrop(findViewById(R.id.pages), bars)
        tilt = Tilt(this) { x, _ ->
            // Leaning right moves the light left, the way a highlight on a real
            // surface does.
            for (bar in bars) bar.light = -x
        }
    }

    /**
     * The history behind or ahead of this page, as a list to jump into.
     *
     * Returns false when there is nothing that way, which leaves the long press
     * unhandled — so the button behaves as if it had simply been held, rather
     * than opening an empty panel.
     */
    private fun showHistoryStrip(backwards: Boolean): Boolean {
        val (pages, index) = tabs.history()
        val entries = if (backwards) {
            HistoryStrip.back(pages, index)
        } else {
            HistoryStrip.forward(pages, index)
        }
        if (entries.isEmpty()) return false

        val rows = entries.map { entry ->
            Panels.Row(
                title = entry.page.label,
                subtitle = UrlResolver.hostOf(entry.page.url),
                onClick = { tabs.goSteps(entry.steps) },
            )
        }
        Panels.list(
            this,
            getString(if (backwards) R.string.back_history else R.string.forward_history),
            rows,
            getString(R.string.no_history),
        ).show()
        return true
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
        // Saved instance state is handed to the system, which may write it to
        // disk. A private window has nothing it is willing to put there.
        if (privacy.isPrivate) return
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
        tilt?.stop()
    }

    override fun onResume() {
        super.onResume()
        tabs.onResume()
        tilt?.start()
        backdrop?.refreshSoon()
    }

    override fun onDestroy() {
        // A WebView outlives its activity if it is left attached, and takes the
        // whole view tree with it.
        backdrop?.stop()
        tilt?.stop()
        tabs.destroy()
        val closing = isFinishing
        super.onDestroy()

        if (closing && !privacy.isPrivate && settings.clearOnExit) {
            // Everything a private window would have thrown away, for someone who
            // wants that from the ordinary one too.
            SettingsPanel.clearEverything(store)
        }

        if (closing && privacy.isPrivate) {
            // Tor goes with the window it was started for.
            if (privacy.usesTor) TorGate.stop()
            PrivateStorage.clearSession(privacy)
            // The process goes with the window. Its data directory is deleted
            // when a private window next starts — deleting it now, with a
            // WebView still winding down inside it, corrupts the profile instead
            // of clearing it. Killing the process is what makes sure nothing in
            // this session outlives the window in memory either.
            Process.killProcess(Process.myPid())
        }
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
        backdrop?.refreshSoon()
    }

    override fun onVisit(url: String, title: String) {
        if (settings.keepHistory) store?.recordVisit(url, title)
        // A new page means new colours behind the glass.
        backdrop?.refreshSoon()
    }

    override fun onTitle(url: String, title: String) {
        if (settings.keepHistory) store?.recordTitle(url, title)
    }

    override fun onPageScrolled() {
        backdrop?.refreshSoon()
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

    // --- Tor -----------------------------------------------------------------

    /**
     * Puts the window behind Tor before it is allowed to load anything.
     *
     * The order matters and is the whole point: start tor, route through it,
     * check through the routed stack, and only then open a page. If any step
     * fails the window closes rather than falling back to the ordinary network —
     * which is what "fails closed" has to mean for this to be worth offering.
     */
    private fun openThroughTor(firstPage: String?) {
        if (!TorGate.isSupported()) {
            AlertDialog.Builder(this)
                .setTitle(R.string.tor_failed_title)
                .setMessage(R.string.tor_unsupported)
                .setCancelable(false)
                .setPositiveButton(android.R.string.ok) { _, _ -> finish() }
                .show()
            return
        }

        val waiting = AlertDialog.Builder(this)
            .setTitle(R.string.tor_connecting)
            .setMessage(getString(R.string.tor_bootstrap, 0) + "\n\n" + getString(R.string.tor_notice))
            .setCancelable(false)
            .create()
        waiting.show()

        // Nothing here has a deadline of its own: tor can bootstrap for as long
        // as it likes, and a window stuck on "building a circuit" with no way
        // forward is the failure that looks most like the app being broken.
        val handler = Handler(Looper.getMainLooper())
        var settled = false
        var progress = 0
        val giveUp = Runnable {
            if (settled) return@Runnable
            settled = true
            waiting.dismiss()
            showTorFailure(
                firstPage,
                getString(R.string.tor_slow, GATE_TIMEOUT_MS / 1000, progress),
            )
        }
        handler.postDelayed(giveUp, GATE_TIMEOUT_MS)

        TorGate.start(this) { state ->
            // States keep arriving after the gate has resolved — tor goes on
            // logging — and acting on them twice would put a second probe
            // WebView behind a dialog that is no longer there.
            if (settled) return@start
            when (state) {
                is TorEngine.State.Starting -> {
                    progress = state.percent
                    waiting.setMessage(
                        getString(R.string.tor_bootstrap, state.percent) + "\n\n" +
                            getString(R.string.tor_notice),
                    )
                }

                is TorEngine.State.Ready -> {
                    settled = true
                    handler.removeCallbacks(giveUp)
                    TorGate.route(state.proxy) {
                        // A WebView of its own, never attached to the window: the
                        // check must go through the same proxied stack the
                        // browsing will use, and it must not leave a page behind
                        // in a tab.
                        val probe = WebView(this)
                        SecureWebView.configure(probe, debuggable = BuildConfig.DEBUG)
                        TorGate.verify(probe) { throughTor ->
                            probe.destroy()
                            waiting.dismiss()
                            if (throughTor) {
                                Toast.makeText(this, R.string.tor_verified, Toast.LENGTH_SHORT).show()
                                firstPage?.let(tabs::open)
                            } else {
                                showTorFailure(firstPage, getString(R.string.tor_check_failed))
                            }
                        }
                    }
                }

                is TorEngine.State.Failed -> {
                    settled = true
                    handler.removeCallbacks(giveUp)
                    waiting.dismiss()
                    showTorFailure(firstPage, state.reason)
                }
            }
        }
    }

    /**
     * What a failed Tor window says, and how to get it out of the phone.
     *
     * The details button is the point. This window blocks screenshots the moment
     * it has a page in it, it runs in its own process, and the interesting part
     * of the failure is usually in a layer that leaves no visible trace — so the
     * report is offered as text to copy or share rather than as something to
     * photograph.
     */
    private fun showTorFailure(firstPage: String?, reason: String) {
        val details = TorEngine.diagnostics(this, reason)
        AlertDialog.Builder(this)
            .setTitle(R.string.tor_failed_title)
            .setMessage(getString(R.string.tor_failed, reason))
            .setCancelable(false)
            .setPositiveButton(R.string.tor_try_again) { _, _ ->
                TorGate.stop()
                openThroughTor(firstPage)
            }
            .setNeutralButton(R.string.tor_details) { _, _ ->
                showTorDetails(firstPage, details)
            }
            .setNegativeButton(R.string.close_private) { _, _ -> finish() }
            .show()
    }

    private fun showTorDetails(firstPage: String?, details: String) {
        AlertDialog.Builder(this)
            .setTitle(R.string.tor_details)
            .setMessage(details)
            .setCancelable(false)
            .setPositiveButton(R.string.copy_details) { _, _ ->
                copy(details)
                showTorFailure(firstPage, getString(R.string.tor_details_copied))
            }
            .setNeutralButton(R.string.share) { _, _ ->
                startActivity(
                    Intent.createChooser(
                        Intent(Intent.ACTION_SEND).apply {
                            type = "text/plain"
                            putExtra(Intent.EXTRA_TEXT, details)
                        },
                        getString(R.string.share),
                    ),
                )
            }
            .setNegativeButton(android.R.string.cancel) { _, _ ->
                showTorFailure(firstPage, details.substringAfter("reason: ").substringBefore('\n'))
            }
            .show()
    }

    // --- the panels ---------------------------------------------------------

    /**
     * The menu, in whatever order the reader has put it.
     *
     * Built from [MenuLayout] rather than from a fixed list, and from ids rather
     * than from positions, so that rearranging it cannot make one item run
     * another's action — the failure that turns a customisable menu into a
     * hazard.
     */
    private fun showMenu(anchor: View) {
        val menu = PopupMenu(this, anchor)
        val layout = MenuLayout.parse(settings.menuLayout)
        val bookmarked = store?.let { shelf ->
            tabs.currentTab()?.url?.let(shelf::isBookmarked)
        } ?: false

        val ids = mutableListOf<String>()
        for (id in layout.visible()) {
            if (!MenuCatalog.appliesTo(id, privacy)) continue
            val title = when (id) {
                "bookmark" -> getString(if (bookmarked) R.string.remove_bookmark else R.string.add_bookmark)
                else -> getString(MenuCatalog.labelRes(id))
            }
            val item = menu.menu.add(0, ids.size, ids.size, title)
            if (id == "desktop") {
                item.isCheckable = true
                item.isChecked = tabs.isDesktopSite
            }
            ids.add(id)
        }

        // Not part of the customisable set: it exists only where it applies, and
        // a private window with no way to close itself is a private window that
        // stays open.
        if (privacy.isPrivate) {
            menu.menu.add(0, ids.size, ids.size, getString(R.string.close_private))
            ids.add("close_private")
        }

        menu.setOnMenuItemClickListener { item ->
            val id = ids.getOrNull(item.itemId) ?: return@setOnMenuItemClickListener false
            when (id) {
                "new_tab" -> tabs.open(settings.homePage)
                "private_cat" -> openPrivateWindow(CatBrowserActivity::class.java)
                "private_lion" -> openPrivateWindow(LionBrowserActivity::class.java)
                "bookmark" -> toggleBookmark()
                "bookmarks" -> showBookmarks()
                "history" -> showHistory()
                "find" -> showFind()
                "share" -> sharePage()
                "downloads" -> DownloadQueue.showAll(this)
                "desktop" -> tabs.setDesktopSite(!tabs.isDesktopSite)
                "customize" -> MenuPanel.show(this, settings) {}.show()
                "settings" -> showSettings()
                "close_private" -> finish()
                else -> return@setOnMenuItemClickListener false
            }
            true
        }
        menu.show()
    }

    /**
     * A private window is a separate task, so it opens beside this one.
     *
     * Below Android 9 there is no `setDataDirectorySuffix`, so a second process
     * cannot have a cookie jar of its own — it would either share this window's
     * or refuse to start. Saying so is the only honest answer; opening a window
     * that says "private" over the ordinary cookies is not.
     */
    private fun openPrivateWindow(window: Class<out BrowserActivity>) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) {
            AlertDialog.Builder(this)
                .setTitle(R.string.private_cat)
                .setMessage(R.string.private_needs_pie)
                .setPositiveButton(android.R.string.ok, null)
                .show()
            return
        }
        startActivity(
            Intent(this, window).addFlags(
                Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_MULTIPLE_TASK,
            ),
        )
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
        val saved = store?.bookmarks() ?: return
        val rows = saved.map { entry ->
            Panels.Row(
                title = entry.label,
                subtitle = entry.url,
                onRemove = { store?.removeBookmark(entry.url) },
                onClick = { load(entry.url) },
            )
        }
        Panels.list(this, getString(R.string.bookmarks), rows, getString(R.string.no_bookmarks)).show()
    }

    private fun showHistory() {
        val shelf = store ?: return
        val visited = shelf.history()
        val rows = visited.map { entry ->
            Panels.Row(title = entry.label, subtitle = entry.url, onClick = { load(entry.url) })
        }
        Panels.list(
            this,
            getString(R.string.history),
            rows,
            getString(R.string.no_history),
            getString(R.string.clear_history) to { shelf.clearHistory() },
        ).show()
    }

    private fun showSettings() {
        val shelf = store ?: return
        SettingsPanel.show(this, settings, shelf) {
            // The preferences that live on a WebView — script, images, text size,
            // Safe Browsing, userscripts — are re-applied to every open tab, and
            // the address bar's suggestions come or go with their setting.
            tabs.refreshSettings()
            address.setAdapter(
                if (settings.suggestFromHistory && privacy.suggestsFromHistory) {
                    SuggestionAdapter(this, shelf)
                } else {
                    null
                },
            )
            // The home page or the engine may have moved; nothing on screen
            // depends on either until the next tap, so there is nothing to redraw.
        }.show()
    }

    // --- find in page --------------------------------------------------------

    private fun showFind() {
        findBar.visibility = View.VISIBLE
        backdrop?.refreshSoon(0)
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
        val shelf = store ?: return
        val tab = tabs.currentTab() ?: return
        if (UrlResolver.decide(tab.url) != UrlResolver.Decision.RENDER) return
        val nowBookmarked = shelf.toggleBookmark(tab.url, tab.title)
        refreshBookmarkButton(tab.url, nowBookmarked)
        Toast.makeText(
            this,
            if (nowBookmarked) R.string.bookmark_added else R.string.bookmark_removed,
            Toast.LENGTH_SHORT,
        ).show()
    }

    private fun refreshBookmarkButton(url: String, known: Boolean? = null) {
        val shelf = store
        if (shelf == null) {
            // A private window has nowhere to keep a bookmark, so the star is not
            // offered rather than offered and ignored.
            bookmark.visibility = View.GONE
            return
        }
        if (known == null) {
            shelf.bookmarked(url) { bookmarked ->
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

    /**
     * The bars are [GlassBar]s and draw their own glass, backdrop and all, so
     * there is nothing to set here but their corners. The small surfaces inside
     * them — the address pill, the tab counter — keep the flat drawable: they sit
     * *on* the glass rather than over the page, and blurring what is already
     * blurred buys nothing.
     */
    private fun applyGlass() {
        findViewById<GlassBar>(R.id.top_bar).cornerRadius =
            GlassSurface.dp(this, GlassGeometry.radiusLarge)
        findViewById<GlassBar>(R.id.bottom_bar).cornerRadius =
            GlassSurface.dp(this, GlassGeometry.radiusLarge)
        findBar.cornerRadius = GlassSurface.dp(this, GlassGeometry.radiusMedium)
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

        /**
         * How long the gate waits for tor before offering a way out.
         *
         * Generous: a first bootstrap on a slow connection genuinely takes a
         * couple of minutes, and giving up early on a Tor window that would have
         * worked is worse than waiting.
         */
        const val GATE_TIMEOUT_MS = 150_000L
    }
}

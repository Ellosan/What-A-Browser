package com.whatabrowser.wat.webview

import android.app.Activity
import android.net.Uri
import android.os.Bundle
import android.os.Message
import android.view.View
import android.view.ViewGroup
import android.webkit.GeolocationPermissions
import android.webkit.PermissionRequest
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebSettings
import android.webkit.WebView
import android.widget.FrameLayout

/**
 * The tabs, as views: one `WebView` each, and no more of them alive than a phone
 * can afford.
 *
 * A live `WebView` is the most expensive object in this app — tens of megabytes
 * of renderer state each. Keeping sixteen of them open on a 4 GB phone is how a
 * browser gets killed in the background and loses everything, so only the few
 * most recently used tabs hold a view; the rest are frozen to a saved state and
 * come back from it. [TabList] decides which, and the reader is meant not to be
 * able to tell — a frozen tab restores its scroll position and its back history.
 *
 * A tab opened in the background costs nothing at all: it has no view and has
 * not loaded until it is looked at.
 */
class Tabs(
    private val activity: Activity,
    private val container: ViewGroup,
    private val settings: Settings,
    private val listener: Listener,
) {

    interface Listener {
        /** The tab in front changed, or its page did. */
        fun onPageChanged(url: String, title: String, loading: Boolean)

        fun onProgress(percent: Int)

        /** The strip changed: a tab opened, closed, or was retitled. */
        fun onTabsChanged()

        fun onVisit(url: String, title: String)

        fun onTitle(url: String, title: String)

        fun onDownloadRequested(
            url: String,
            userAgent: String?,
            contentDisposition: String?,
            mimeType: String?,
        )

        fun onFileChooser(
            callback: ValueCallback<Array<Uri>>,
            params: WebChromeClient.FileChooserParams,
        ): Boolean

        fun onFindResult(activeMatch: Int, matches: Int)

        /** A long press on the page, with what was underneath it. */
        fun onLongPress(type: Int, target: String?): Boolean

        fun onFullscreen(view: View?, exit: WebChromeClient.CustomViewCallback?)

        fun onTabLimitReached()
    }

    val list = TabList(MAX_TABS)

    /** Tabs with a view. Never larger than [LIVE_TABS]. */
    private val live = HashMap<Int, WebView>()

    /** Tabs without one, and the state to bring them back from. */
    private val frozen = HashMap<Int, Bundle>()

    /** Which tabs are asking for the desktop site. */
    private val desktop = HashSet<Int>()

    private val defaultUserAgent: String by lazy { WebSettings.getDefaultUserAgent(activity) }

    val count: Int get() = list.count

    fun current(): WebView? = list.currentId?.let { live[it] }

    fun currentTab(): TabList.Tab? = list.current

    fun all(): List<TabList.Tab> = list.all()

    // --- opening, selecting, closing --------------------------------------

    /** Opens a tab. A background tab does not load until it is selected. */
    fun open(url: String, foreground: Boolean = true) {
        val tab = list.open(url, foreground)
        if (tab == null) {
            listener.onTabLimitReached()
            return
        }
        if (foreground) show(tab) else listener.onTabsChanged()
    }

    fun select(id: Int) {
        list.select(id)?.let(::show)
    }

    /**
     * Closes a tab. Returns false when that was the last one, which the caller
     * answers by opening a fresh home page rather than showing an empty browser.
     */
    fun close(id: Int): Boolean {
        discard(id)
        frozen.remove(id)
        desktop.remove(id)
        val next = list.close(id)
        if (next == null) {
            listener.onTabsChanged()
            return false
        }
        show(next)
        return true
    }

    fun closeAll() {
        list.all().forEach { discard(it.id) }
        frozen.clear()
        desktop.clear()
        list.closeAll()
    }

    /** Brings a tab forward, creating or thawing its view as needed. */
    private fun show(tab: TabList.Tab) {
        val view = live[tab.id] ?: thaw(tab)
        for ((id, other) in live) {
            val front = id == tab.id
            other.visibility = if (front) View.VISIBLE else View.GONE
            if (front) other.onResume() else other.onPause()
        }
        // Freezing happens after the new tab has its view, so the tab being
        // shown is never the one given up.
        enforceLiveLimit()
        listener.onPageChanged(view.url ?: tab.url, tab.title, false)
        listener.onTabsChanged()
    }

    private fun thaw(tab: TabList.Tab): WebView {
        val view = createView(tab.id)
        val state = frozen.remove(tab.id)
        val restored = state?.let { view.restoreState(it) }
        if (restored == null) {
            // A tab that has never been loaded, or whose state did not survive.
            if (UrlResolver.decide(tab.url) == UrlResolver.Decision.RENDER) view.loadUrl(tab.url)
        }
        return view
    }

    private fun enforceLiveLimit() {
        while (live.size > LIVE_TABS) {
            val victim = list.leastRecentlyUsed(live.keys) ?: break
            freeze(victim)
        }
    }

    private fun freeze(id: Int) {
        val view = live[id] ?: return
        Bundle().let { state ->
            if (view.saveState(state) != null) frozen[id] = state
        }
        discard(id)
    }

    private fun discard(id: Int) {
        val view = live.remove(id) ?: return
        container.removeView(view)
        view.destroy()
    }

    // --- the view itself ---------------------------------------------------

    private fun createView(id: Int): WebView {
        val view = WebView(activity)
        view.layoutParams = FrameLayout.LayoutParams(
            FrameLayout.LayoutParams.MATCH_PARENT,
            FrameLayout.LayoutParams.MATCH_PARENT,
        )
        SecureWebView.configure(view, debuggable = BuildConfig.DEBUG)

        if (settings.desktopSite) desktop.add(id)
        applyUserAgent(view, id in desktop)

        view.webViewClient = BrowserClient(activity) { url, loading ->
            onNavigation(id, url, loading)
        }
        view.webChromeClient = ChromeClient(id)

        // The one place a download begins: a response the WebView cannot display.
        view.setDownloadListener { url, userAgent, contentDisposition, mimeType, _ ->
            listener.onDownloadRequested(url, userAgent, contentDisposition, mimeType)
        }
        view.setFindListener { activeMatchOrdinal, numberOfMatches, _ ->
            if (id == list.currentId) listener.onFindResult(activeMatchOrdinal, numberOfMatches)
        }
        view.setOnLongClickListener {
            // `hitTestResult` is how a WebView says what is under the finger.
            // Asked for here rather than in the activity because the answer
            // belongs to a particular view, and there is more than one.
            val hit = view.hitTestResult
            listener.onLongPress(hit.type, hit.extra)
        }

        live[id] = view
        container.addView(view)
        return view
    }

    /**
     * A window the page opened itself, which becomes a tab.
     *
     * Its view exists but has loaded nothing: the WebView that asked for it does
     * the loading, which is the only way `window.open` can work at all.
     */
    private fun openForWindow(): WebView? {
        val tab = list.open("", foreground = true) ?: run {
            listener.onTabLimitReached()
            return null
        }
        val view = createView(tab.id)
        for ((id, other) in live) {
            val front = id == tab.id
            other.visibility = if (front) View.VISIBLE else View.GONE
            if (front) other.onResume() else other.onPause()
        }
        enforceLiveLimit()
        listener.onTabsChanged()
        return view
    }

    private fun onNavigation(id: Int, url: String, loading: Boolean) {
        val tab = list.byId(id) ?: return
        if (url.isNotEmpty()) tab.url = url
        if (!loading) listener.onVisit(url, tab.title)
        if (id != list.currentId) return
        listener.onPageChanged(url, tab.title, loading)
    }

    // --- what the interface asks of the current tab -------------------------

    fun goBack(): Boolean {
        val view = current() ?: return false
        if (!view.canGoBack()) return false
        view.goBack()
        return true
    }

    fun goForward() {
        current()?.takeIf { it.canGoForward() }?.goForward()
    }

    fun reload() {
        current()?.reload()
    }

    fun stopLoading() {
        current()?.stopLoading()
    }

    fun load(url: String) {
        // A tab whose view has been frozen still has to be loadable: the address
        // bar belongs to the tab in front, not to a live WebView.
        val tab = list.current ?: run {
            open(url)
            return
        }
        tab.url = url
        (live[tab.id] ?: thaw(tab)).loadUrl(url)
    }

    fun find(text: String) {
        if (text.isEmpty()) current()?.clearMatches() else current()?.findAllAsync(text)
    }

    fun findNext(forward: Boolean) {
        current()?.findNext(forward)
    }

    fun clearFind() {
        current()?.clearMatches()
    }

    val isDesktopSite: Boolean get() = list.currentId?.let { it in desktop } ?: settings.desktopSite

    /** Applies to this tab only, the way every other browser does it. */
    fun setDesktopSite(enabled: Boolean) {
        val id = list.currentId ?: return
        if (enabled) desktop.add(id) else desktop.remove(id)
        val view = live[id] ?: return
        applyUserAgent(view, enabled)
        view.reload()
    }

    private fun applyUserAgent(view: WebView, asDesktop: Boolean) {
        view.settings.userAgentString = if (asDesktop) {
            UserAgents.desktop(defaultUserAgent)
        } else {
            null // the platform's own, which is the honest one on a phone
        }
        // A desktop page laid out at phone width is unreadable; zoomed out to fit
        // it is at least the page the reader asked for.
        view.settings.loadWithOverviewMode = true
        view.settings.useWideViewPort = true
    }

    // --- lifecycle ---------------------------------------------------------

    fun onPause() {
        current()?.onPause()
    }

    fun onResume() {
        current()?.onResume()
    }

    fun destroy() {
        closeAll()
    }

    /**
     * Saves the session across a rotation or a process death.
     *
     * The full back history is saved for the tab in front only. Every tab's
     * history would be more faithful and is not worth the risk: saved instance
     * state crosses a 1 MB binder transaction, and a browser that crashes on
     * rotation with a dozen tabs open is worse than one that forgets the back
     * button in the eleven the reader is not looking at.
     */
    fun saveInstanceState(out: Bundle) {
        val tabs = list.all()
        out.putIntArray(KEY_IDS, tabs.map { it.id }.toIntArray())
        out.putStringArray(KEY_URLS, tabs.map { it.url }.toTypedArray())
        out.putStringArray(KEY_TITLES, tabs.map { it.title }.toTypedArray())
        out.putIntArray(KEY_DESKTOP, desktop.toIntArray())
        list.currentId?.let { out.putInt(KEY_CURRENT, it) }
        current()?.let { view ->
            val state = Bundle()
            if (view.saveState(state) != null) out.putBundle(KEY_CURRENT_STATE, state)
        }
    }

    /** Returns false when there was nothing to restore. */
    fun restoreInstanceState(state: Bundle): Boolean {
        val ids = state.getIntArray(KEY_IDS) ?: return false
        val urls = state.getStringArray(KEY_URLS) ?: return false
        val titles = state.getStringArray(KEY_TITLES) ?: return false
        if (ids.isEmpty() || urls.size != ids.size || titles.size != ids.size) return false

        val entries = ids.indices.map { TabList.Tab(ids[it], urls[it], titles[it]) }
        val selected = if (state.containsKey(KEY_CURRENT)) state.getInt(KEY_CURRENT) else null
        list.restore(entries, selected)
        desktop.clear()
        state.getIntArray(KEY_DESKTOP)?.forEach(desktop::add)

        val tab = list.current ?: return false
        val view = createView(tab.id)
        val saved = state.getBundle(KEY_CURRENT_STATE)
        if (saved == null || view.restoreState(saved) == null) {
            if (UrlResolver.decide(tab.url) == UrlResolver.Decision.RENDER) view.loadUrl(tab.url)
        }
        view.visibility = View.VISIBLE
        listener.onPageChanged(tab.url, tab.title, false)
        listener.onTabsChanged()
        return true
    }

    /**
     * Everything the page is allowed to ask the app for, and the answer.
     *
     * One per tab, so a background tab's progress and title cannot be mistaken
     * for the foreground one's.
     */
    private inner class ChromeClient(private val id: Int) : WebChromeClient() {

        override fun onProgressChanged(view: WebView, newProgress: Int) {
            if (id == list.currentId) listener.onProgress(newProgress)
        }

        override fun onReceivedTitle(view: WebView, title: String?) {
            val tab = list.byId(id) ?: return
            tab.title = title.orEmpty()
            view.url?.let { listener.onTitle(it, tab.title) }
            if (id == list.currentId) listener.onPageChanged(view.url ?: tab.url, tab.title, false)
            listener.onTabsChanged()
        }

        /**
         * `target="_blank"` and `window.open`, which become tabs.
         *
         * Only on a real gesture. `javaScriptCanOpenWindowsAutomatically` is off,
         * so this should never be called without one, but a popup is exactly the
         * thing worth refusing twice.
         */
        override fun onCreateWindow(
            view: WebView,
            isDialog: Boolean,
            isUserGesture: Boolean,
            resultMsg: Message,
        ): Boolean {
            if (!isUserGesture) return false
            val child = openForWindow() ?: return false
            (resultMsg.obj as? WebView.WebViewTransport)?.webView = child
            resultMsg.sendToTarget()
            return true
        }

        override fun onCloseWindow(window: WebView) {
            val id = live.entries.firstOrNull { it.value === window }?.key ?: return
            if (!close(id)) open(settings.homePage)
        }

        /**
         * Camera and microphone: refused, always.
         *
         * The app holds neither permission, so granting one would fail anyway —
         * but answering explicitly means a page cannot leave the request hanging
         * and then claim the browser is broken.
         */
        override fun onPermissionRequest(request: PermissionRequest) {
            request.deny()
        }

        override fun onGeolocationPermissionsShowPrompt(
            origin: String,
            callback: GeolocationPermissions.Callback,
        ) {
            callback.invoke(origin, false, false)
        }

        override fun onShowFileChooser(
            webView: WebView,
            filePathCallback: ValueCallback<Array<Uri>>,
            fileChooserParams: FileChooserParams,
        ): Boolean = listener.onFileChooser(filePathCallback, fileChooserParams)

        // Video that fills the screen. Without these the fullscreen button on
        // every video site does nothing at all.
        override fun onShowCustomView(view: View, callback: CustomViewCallback) {
            listener.onFullscreen(view, callback)
        }

        override fun onHideCustomView() {
            listener.onFullscreen(null, null)
        }

        // The page's own dialogs, drawn by the browser rather than by the page,
        // and always saying which site is asking.
        override fun onJsAlert(
            view: WebView,
            url: String?,
            message: String?,
            result: android.webkit.JsResult,
        ): Boolean = JsDialogs.alert(activity, url, message, result)

        override fun onJsConfirm(
            view: WebView,
            url: String?,
            message: String?,
            result: android.webkit.JsResult,
        ): Boolean = JsDialogs.confirm(activity, url, message, result)

        override fun onJsPrompt(
            view: WebView,
            url: String?,
            message: String?,
            defaultValue: String?,
            result: android.webkit.JsPromptResult,
        ): Boolean = JsDialogs.prompt(activity, url, message, defaultValue, result)

        override fun onJsBeforeUnload(
            view: WebView,
            url: String?,
            message: String?,
            result: android.webkit.JsResult,
        ): Boolean = JsDialogs.beforeUnload(activity, url, result)
    }

    private companion object {
        /** More than anyone browses on a phone, and few enough to stay listable. */
        const val MAX_TABS = 16

        /**
         * How many tabs keep a live view. Three is the foreground tab plus the two
         * most likely to be switched back to; a fourth buys very little and costs
         * another renderer.
         */
        const val LIVE_TABS = 3

        const val KEY_IDS = "tab_ids"
        const val KEY_URLS = "tab_urls"
        const val KEY_TITLES = "tab_titles"
        const val KEY_DESKTOP = "tab_desktop"
        const val KEY_CURRENT = "tab_current"
        const val KEY_CURRENT_STATE = "tab_current_state"
    }
}

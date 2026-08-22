package com.whatabrowser.wat.webview

import android.content.Context
import android.webkit.WebView
import androidx.webkit.WebViewCompat
import androidx.webkit.WebViewFeature

/**
 * Running the scripts: what gets injected, and when.
 *
 * This is the answer to "extensions" for a browser that cannot have them. An
 * extension is a program with privileges over the browser; a userscript is
 * JavaScript that runs inside one page, with the page's own privileges and no
 * more. That is a much smaller thing, and it is a thing a WebView can do
 * honestly — which is why this exists and an extension system does not.
 *
 * Two moments to inject at, because scripts ask for both:
 *
 * - `document-start`, through `addDocumentStartJavaScript`, which the engine
 *   runs before the page's own scripts. Its origin rules are host-level, so the
 *   source is also wrapped in a check on `location.href` built from the same
 *   patterns — the rule decides which origins it is registered for, the check
 *   decides whether this particular address matches.
 * - `document-end`, evaluated when the page has finished, where the address is
 *   already known and Kotlin can do the matching exactly.
 *
 * Everything injected is wrapped so that a script's own variables cannot leak
 * into the page, and a script that throws cannot stop the page.
 */
class UserScripts(context: Context, private val settings: Settings) {

    private val store = UserScriptStore(context)

    private val prelude: String by lazy {
        runCatching {
            context.assets.open("userscript-prelude.js").use { it.readBytes().decodeToString() }
        }.getOrDefault("")
    }

    /** Registered start-of-document injections, so they can be undone. */
    private val registered = mutableListOf<androidx.webkit.ScriptHandler>()

    val enabled: Boolean get() = settings.userScripts

    fun installed(): List<UserScriptStore.Installed> = store.all()

    fun store(): UserScriptStore = store

    /**
     * Registers every enabled start-of-document script on a fresh view.
     *
     * Called once per WebView. Undone by [clear] when the setting changes, since
     * a registration outlives the page.
     */
    fun attach(webView: WebView) {
        if (!enabled) return
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.DOCUMENT_START_SCRIPT)) return

        for (installed in store.all()) {
            if (!installed.enabled || !installed.script.runAtStart) continue
            val rules = originRules(installed.script)
            runCatching {
                registered += WebViewCompat.addDocumentStartJavaScript(
                    webView,
                    wrap(installed.script, guarded = true),
                    rules,
                )
            }
        }
    }

    fun clear() {
        for (handler in registered) {
            runCatching { handler.remove() }
        }
        registered.clear()
    }

    /**
     * Runs the end-of-document scripts for a page that has finished loading.
     *
     * `evaluateJavascript` is the app calling into the page, which is the safe
     * direction; there is still no JavaScript interface anywhere in this browser,
     * so nothing in the page can call back out.
     */
    fun onPageFinished(webView: WebView, url: String) {
        if (!enabled) return
        for (script in store.forUrl(url)) {
            if (script.runAtStart) continue
            runCatching { webView.evaluateJavascript(wrap(script, guarded = false), null) }
        }
    }

    /**
     * The origins a start-of-document script may be registered for.
     *
     * A script whose patterns cover everything gets the everywhere rule, which is
     * what it asked for; anything narrower is listed origin by origin.
     */
    private fun originRules(script: UserScript): Set<String> {
        val rules = script.matches.map(MatchPattern::originRule)
        return if (rules.isEmpty() || rules.any { it == null }) {
            setOf("*")
        } else {
            rules.filterNotNull().toSet()
        }
    }

    /**
     * One script, ready to inject: the shim, then the script, inside a closure
     * and a try/catch, and behind an address check when the caller cannot do the
     * matching itself.
     */
    private fun wrap(script: UserScript, guarded: Boolean): String {
        val prelude = prelude
            .replace("__WAT_SCRIPT_NAME__", jsString(script.name))
            .replace("__WAT_SCRIPT_VERSION__", jsString(script.version))
        val body = buildString {
            append("(function(){\n")
            if (guarded) {
                append("if (!(").append(MatchPattern.jsGuard(script.matches)).append(")) return;\n")
            }
            append("try {\n")
            append(prelude)
            append("\n")
            append(script.source)
            append("\n} catch (error) { try { console.error('userscript ")
            append(script.name.replace("'", ""))
            append(" failed', error); } catch (ignored) {} }\n")
            append("})();")
        }
        return body
    }

    private fun jsString(text: String): String = buildString {
        append('"')
        for (char in text) {
            when {
                char == '"' -> append("\\\"")
                char == '\\' -> append("\\\\")
                char.code < 0x20 || char.code > 0x7E -> append("\\u").append("%04x".format(char.code))
                else -> append(char)
            }
        }
        append('"')
    }
}

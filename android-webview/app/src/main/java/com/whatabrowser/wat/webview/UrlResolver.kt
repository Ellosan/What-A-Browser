package com.whatabrowser.wat.webview

import java.net.URLEncoder

/**
 * Turns what a person typed into something to load, and decides what the browser
 * is willing to load at all.
 *
 * Deliberately free of `android.net.Uri` and every other platform type, so it
 * runs in an ordinary JVM test. That matters more here than it looks: the scheme
 * policy below is a security boundary, and a boundary only ever exercised by hand
 * on a device is one nobody has actually checked.
 */
object UrlResolver {

    /**
     * The only schemes the WebView is allowed to render.
     *
     * Everything else is refused. `file:` and `content:` because a page that can
     * reach them can read the app's own storage and the user's documents;
     * `javascript:` and `data:` because they are how a link gets script running
     * in the context of the page already open; `intent:` because it launches
     * other apps with extras the page chooses.
     */
    private val RENDERABLE = setOf("http", "https")

    /** Handed to the system rather than rendered. */
    private val HANDOFF = setOf("mailto", "tel", "sms")

    enum class Decision { RENDER, ASK_SYSTEM, REFUSE }

    fun decide(url: String): Decision = when (schemeOf(url)) {
        in RENDERABLE -> Decision.RENDER
        in HANDOFF -> Decision.ASK_SYSTEM
        else -> Decision.REFUSE
    }

    /**
     * What to load for something typed into the address bar.
     *
     * A bare host becomes `https://`, never `http://`: guessing cleartext is how
     * a typed address turns into a downgrade. Anything not recognisable as an
     * address becomes a search.
     */
    fun resolve(input: String, searchTemplate: String): String {
        val trimmed = input.trim()
        if (trimmed.isEmpty()) return ""

        val scheme = schemeOf(trimmed)
        if (scheme != null) {
            // A typed scheme is held to the same policy as a link. Pasting
            // `javascript:...` into an address bar is the oldest trick there is,
            // so it becomes a search rather than something to run.
            return if (scheme in RENDERABLE || scheme in HANDOFF) {
                trimmed
            } else {
                search(trimmed, searchTemplate)
            }
        }

        return if (looksLikeHost(trimmed)) "https://$trimmed" else search(trimmed, searchTemplate)
    }

    /**
     * The scheme of `url`, lowercased, or null if it has none.
     *
     * Follows RFC 3986: a letter followed by letters, digits, `+`, `-` or `.`,
     * then a colon. Parsed by hand rather than with `Uri`, which accepts things
     * this should not — and it is safer for a security check to be strict and
     * obvious than lenient and clever.
     */
    private fun schemeOf(url: String): String? {
        val colon = url.indexOf(':')
        if (colon <= 0) return null
        val candidate = url.substring(0, colon)
        if (!candidate[0].isLetter()) return null
        if (!candidate.all { it.isLetterOrDigit() || it == '+' || it == '-' || it == '.' }) return null

        // `localhost:8080` is a host and a port, not a scheme called "localhost".
        // Nothing else distinguishes the two, so a colon followed by digits and
        // then nothing or a path is read as a port.
        val rest = url.substring(colon + 1)
        val port = rest.takeWhile { it.isDigit() }
        if (port.isNotEmpty() && (port.length == rest.length || rest[port.length] == '/')) return null

        return candidate.lowercase()
    }

    private fun search(terms: String, template: String): String =
        template.replace("{}", URLEncoder.encode(terms, "UTF-8"))

    /**
     * A single token with a dot in the middle and no spaces reads as a host.
     *
     * Deliberately conservative. Treating "not obviously a search" as an address
     * sends whatever was typed to a DNS server as a hostname, and people type
     * private things into address bars.
     */
    private fun looksLikeHost(text: String): Boolean {
        if (text.any { it.isWhitespace() }) return false
        if (text == "localhost" || text.startsWith("localhost/") || text.startsWith("localhost:")) {
            return true
        }
        val host = text.substringBefore('/').substringBefore('?')
        val dot = host.indexOf('.')
        return dot > 0 && dot < host.length - 1
    }
}

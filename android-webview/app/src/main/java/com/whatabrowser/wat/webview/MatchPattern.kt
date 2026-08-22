package com.whatabrowser.wat.webview

/**
 * The `@match` patterns a userscript uses to say where it runs.
 *
 * Chrome's match-pattern grammar, which every userscript manager follows: a
 * scheme, then a host, then a path. The scheme may be a star, the host may begin
 * with a star and a dot, and the path may contain stars; a star in all three
 * places means everywhere.
 *
 * The patterns are not written out in these comments on purpose — Kotlin nests
 * block comments, so the slash-star inside one would open a comment that ate the
 * terminator. `MatchPatternTest` has them all, in strings, where they belong.
 *
 * It matters that this is strict. A pattern is the only thing standing between a
 * script the reader installed for one site and every other site they visit,
 * including their bank — so a pattern that cannot be parsed matches nothing, and
 * a host wildcard only ever stands for whole labels.
 */
object MatchPattern {

    private const val ANY = "*"

    class Pattern(
        private val scheme: String,
        private val host: String,
        private val path: String,
    ) {
        fun matches(url: String): Boolean {
            val schemeEnd = url.indexOf("://")
            if (schemeEnd <= 0) return false
            val urlScheme = url.substring(0, schemeEnd).lowercase()
            val rest = url.substring(schemeEnd + 3)
            val slash = rest.indexOf('/')
            val authority = if (slash < 0) rest else rest.substring(0, slash)
            val urlPath = if (slash < 0) "/" else rest.substring(slash)
            // Credentials and ports are not part of the host being matched.
            val urlHost = authority.substringAfterLast('@').substringBefore(':').lowercase()

            // A star for the scheme means http or https, as it does in Chrome —
            // not "any scheme at all". Without this, the everywhere pattern
            // would also mean `file://`, and a script installed for the web
            // would run against local files.
            if (scheme == ANY) {
                if (urlScheme != "http" && urlScheme != "https") return false
            } else if (scheme != urlScheme) {
                return false
            }
            if (urlHost.isEmpty()) return false
            if (!hostMatches(urlHost)) return false
            return globMatches(path, urlPath.substringBefore('#'))
        }

        private fun hostMatches(urlHost: String): Boolean = when {
            host == ANY -> true
            host.startsWith("*.") -> {
                val suffix = host.substring(2)
                // A label boundary, so `*.example.com` does not match
                // `notexample.com`.
                urlHost == suffix || urlHost.endsWith(".$suffix")
            }
            else -> urlHost == host
        }
    }

    /**
     * The origin this pattern covers, in the form
     * `WebViewCompat.addDocumentStartJavaScript` wants, or null if it covers
     * every origin.
     *
     * Those rules are host-level: they cannot express a path. So they are the
     * outer of two fences — this decides which origins a start-of-document
     * script is registered for at all, and [jsGuard] decides, inside the page,
     * whether the address actually matches. Either alone would be too coarse or
     * too late.
     */
    fun originRule(pattern: String): String? {
        val trimmed = pattern.trim()
        if (trimmed == "<all_urls>") return null
        val schemeEnd = trimmed.indexOf("://")
        if (schemeEnd <= 0) return null
        val scheme = trimmed.substring(0, schemeEnd)
        val host = trimmed.substring(schemeEnd + 3).substringBefore('/')
        if (host == ANY) return null
        return scheme + "://" + host
    }

    /**
     * A JavaScript expression that is true on exactly the addresses these
     * patterns match.
     *
     * Used to fence a start-of-document script inside the page, where the path is
     * known and the host-level origin rule is not enough on its own. Everything
     * is escaped and the wildcards put back afterwards, so a pattern cannot
     * inject anything into the expression it produces — the patterns come from a
     * file the reader installed, and that file is the thing being fenced.
     */
    fun jsGuard(patterns: List<String>): String {
        val usable = patterns.map { it.trim() }.filter { parse(it) != null }
        if (usable.isEmpty()) return "false"
        return usable.joinToString(" || ") { pattern ->
            "new RegExp(" + jsString(regexFor(pattern)) + ").test(location.href)"
        }
    }

    private fun regexFor(pattern: String): String {
        if (pattern == "<all_urls>") return "^https?://.*$"
        val schemeEnd = pattern.indexOf("://")
        val scheme = pattern.substring(0, schemeEnd)
        val rest = pattern.substring(schemeEnd + 3)
        val host = rest.substringBefore('/')
        val path = rest.substring(host.length).ifEmpty { "/" + ANY }

        val schemePart = if (scheme == ANY) "https?" else quote(scheme)
        val hostPart = when {
            host == ANY -> "[^/]+"
            host.startsWith("*.") -> "(?:[^/]+\\.)?" + quote(host.substring(2))
            else -> quote(host)
        }
        val portPart = "(?::[0-9]+)?"
        val pathPart = path.split(ANY).joinToString(".*") { quote(it) }
        return "^" + schemePart + "://" + hostPart + portPart + pathPart + "$"
    }

    /**
     * A literal, as a regular expression.
     *
     * Everything that is not a letter or a digit is escaped, which is heavy
     * handed and exactly right: the alternative is a list of metacharacters to
     * keep in step with two regex dialects.
     */
    private fun quote(text: String): String = buildString {
        for (char in text) {
            if (char.isLetterOrDigit()) append(char) else append('\\').append(char)
        }
    }

    /** A JavaScript string literal, so a pattern cannot escape the expression. */
    private fun jsString(text: String): String = buildString {
        append('"')
        for (char in text) {
            when {
                char == '"' -> append("\\\"")
                char == '\\' -> append("\\\\")
                char.code < 0x20 || char.code > 0x7E ->
                    append("\\u").append("%04x".format(char.code))
                else -> append(char)
            }
        }
        append('"')
    }

    /** Null for anything that is not a pattern, which then matches nothing. */
    fun parse(pattern: String): Pattern? {
        val trimmed = pattern.trim()
        if (trimmed.isEmpty()) return null
        // `<all_urls>` is the other spelling of everywhere.
        if (trimmed == "<all_urls>") return Pattern(ANY, ANY, "/*")

        val schemeEnd = trimmed.indexOf("://")
        if (schemeEnd <= 0) return null
        val scheme = trimmed.substring(0, schemeEnd).lowercase()
        if (scheme != ANY && scheme != "http" && scheme != "https") return null

        val rest = trimmed.substring(schemeEnd + 3)
        if (rest.isEmpty()) return null
        val slash = rest.indexOf('/')
        val host = (if (slash < 0) rest else rest.substring(0, slash)).lowercase()
        val path = if (slash < 0) "/*" else rest.substring(slash)
        if (host.isEmpty()) return null
        // A host is letters, digits, dots and dashes — and the star. Anything
        // else is not a host, and the patterns come from a file the reader
        // installed, so the strict reading is the right one.
        if (!host.all { it.isLetterOrDigit() || it == '.' || it == '-' || it == '*' }) return null
        // A star anywhere but as a whole leading label is not a host pattern.
        if (host != ANY && host.contains(ANY) && !host.startsWith("*.")) return null
        if (host.startsWith("*.") && host.substring(2).contains(ANY)) return null

        return Pattern(scheme, host, path)
    }

    /** `*` stands for any run of characters, including none. Nothing else is special. */
    private fun globMatches(pattern: String, text: String): Boolean {
        val parts = pattern.split(ANY)
        if (parts.size == 1) return text == pattern

        if (!text.startsWith(parts.first())) return false
        if (!text.endsWith(parts.last())) return false

        var at = parts.first().length
        for (part in parts.subList(1, parts.size - 1)) {
            if (part.isEmpty()) continue
            val found = text.indexOf(part, at)
            if (found < 0) return false
            at = found + part.length
        }
        // The trailing piece has to fit after everything already consumed.
        return at <= text.length - parts.last().length
    }
}

package com.whatabrowser.wat.webview

/**
 * A userscript, and the metadata block at the top of it.
 *
 * This is the alternative to extensions. A WebView has no extension platform at
 * all — that is what the Chromium fork in `chromium/` exists for — but a
 * userscript is just JavaScript with a header saying where to run it, and
 * injecting it is something a WebView can do properly.
 *
 * Parsing is here, away from Android, because the `@match` lines decide which
 * sites someone else's code runs on. That is the whole security boundary of the
 * feature, and it is worth tests.
 */
class UserScript(
    val name: String,
    val version: String,
    val description: String,
    val matches: List<String>,
    val excludes: List<String>,
    val runAtStart: Boolean,
    val grants: List<String>,
    val source: String,
) {

    private val patterns = matches.mapNotNull(MatchPattern::parse)
    private val exclusions = excludes.mapNotNull(MatchPattern::parse)

    /**
     * Whether this script runs on [url].
     *
     * A script with no usable `@match` runs nowhere. Userscript managers treat a
     * missing match as "ask", and there is nobody to ask here — so the safe
     * reading is the strict one.
     */
    fun appliesTo(url: String): Boolean {
        if (patterns.isEmpty()) return false
        if (UrlResolver.decide(url) != UrlResolver.Decision.RENDER) return false
        if (exclusions.any { it.matches(url) }) return false
        return patterns.any { it.matches(url) }
    }

    /** The `@grant`s asked for that this browser does not provide. */
    fun unsupportedGrants(): List<String> = grants.filterNot { it in SUPPORTED_GRANTS || it == "none" }

    companion object {

        /**
         * What the shim in `assets/userscript-prelude.js` implements.
         *
         * Everything else a script asks for is missing, and [unsupportedGrants]
         * reports it rather than letting the script fail halfway through with a
         * reference error nobody sees.
         */
        val SUPPORTED_GRANTS = setOf(
            "GM_addStyle",
            "GM_setValue",
            "GM_getValue",
            "GM_deleteValue",
            "GM_listValues",
            "GM_registerMenuCommand",
            "GM_info",
            "GM_log",
        )

        /** As long as a script is allowed to be. Enough for anything hand-written. */
        const val MAX_SOURCE = 512 * 1024

        /**
         * Reads the `==UserScript==` block. Null if there is not one, or if the
         * source is too long to be something a person wrote.
         */
        fun parse(source: String): UserScript? {
            if (source.length > MAX_SOURCE) return null
            val start = source.indexOf("// ==UserScript==")
            if (start < 0) return null
            val end = source.indexOf("// ==/UserScript==", start)
            if (end < 0) return null

            val fields = mutableMapOf<String, MutableList<String>>()
            for (line in source.substring(start, end).lines()) {
                val trimmed = line.trim().removePrefix("//").trim()
                if (!trimmed.startsWith("@")) continue
                val key = trimmed.substringBefore(' ').substringBefore('\t').removePrefix("@")
                val value = trimmed.removePrefix("@$key").trim()
                if (key.isEmpty()) continue
                fields.getOrPut(key) { mutableListOf() }.add(value)
            }

            val name = fields["name"]?.firstOrNull()?.takeIf { it.isNotBlank() } ?: return null
            return UserScript(
                name = name,
                version = fields["version"]?.firstOrNull().orEmpty(),
                description = fields["description"]?.firstOrNull().orEmpty(),
                // `@include` is the older spelling, and scripts still use both.
                matches = (fields["match"].orEmpty() + fields["include"].orEmpty())
                    .filter { it.isNotBlank() },
                excludes = fields["exclude"].orEmpty().filter { it.isNotBlank() },
                runAtStart = fields["run-at"]?.firstOrNull()?.trim() == "document-start",
                grants = fields["grant"].orEmpty().map { it.trim() }.filter { it.isNotBlank() },
                source = source,
            )
        }
    }
}

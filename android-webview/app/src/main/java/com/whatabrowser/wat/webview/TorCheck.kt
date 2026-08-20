package com.whatabrowser.wat.webview

/**
 * Reading the answer from `https://check.torproject.org/api/ip`.
 *
 * Tor mode is worth nothing if it is not actually going through Tor, so the mode
 * does not open until the check has passed. The parsing is here, away from
 * Android, because the safe answer to anything unexpected is "no" and that is a
 * property worth having tests for: a garbled response, an error page, an empty
 * string and a captive portal must all read as not-Tor.
 */
object TorCheck {

    fun isTor(response: String?): Boolean {
        val body = unquote(response ?: return false)
        // Matching the field rather than parsing the document: the answer arrives
        // as whatever the page happened to contain, and a JSON parser that throws
        // on an error page would need the same fallback anyway.
        val match = Regex(""""IsTor"\s*:\s*(true|false)""").find(body) ?: return false
        return match.groupValues[1] == "true"
    }

    /**
     * `evaluateJavascript` hands back a JSON *value*, so a string arrives quoted
     * and escaped: `"{\"IsTor\":true}"`. Unwrapped here rather than by a JSON
     * library, since this is the only shape that ever needs it.
     */
    private fun unquote(raw: String): String {
        val trimmed = raw.trim()
        if (trimmed.length < 2 || !trimmed.startsWith("\"") || !trimmed.endsWith("\"")) return trimmed
        return trimmed.substring(1, trimmed.length - 1)
            .replace("\\\"", "\"")
            .replace("\\n", "\n")
            .replace("\\/", "/")
            .replace("\\\\", "\\")
    }
}

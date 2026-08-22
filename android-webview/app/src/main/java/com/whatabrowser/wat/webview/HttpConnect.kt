package com.whatabrowser.wat.webview

/**
 * The one HTTP request the bridge accepts: `CONNECT host:port HTTP/1.1`.
 *
 * This is what a browser sends an HTTP proxy when it wants a tunnel for HTTPS,
 * and since this browser refuses cleartext, it is the only thing the WebView will
 * ever ask for. Everything else is turned away rather than forwarded, so the
 * bridge cannot be talked into fetching a URL on someone's behalf.
 *
 * Parsed strictly. This listens on a local port that any app on the device can
 * reach, so the request line is untrusted input.
 */
object HttpConnect {

    class Target(val host: String, val port: Int)

    /** The longest request line accepted, so a socket cannot be used to fill memory. */
    const val MAX_LINE = 2048

    fun parse(line: String): Target? {
        if (line.length > MAX_LINE) return null

        val parts = line.trim().split(' ').filter { it.isNotEmpty() }
        if (parts.size < 2) return null
        if (!parts[0].equals("CONNECT", ignoreCase = true)) return null

        val authority = parts[1]
        // An IPv6 literal is bracketed, and its colons are part of the address.
        val separator = if (authority.startsWith("[")) {
            val close = authority.indexOf(']')
            if (close < 0) return null
            authority.indexOf(':', close)
        } else {
            authority.lastIndexOf(':')
        }
        if (separator <= 0) return null

        val host = authority.substring(0, separator)
        val port = authority.substring(separator + 1).toIntOrNull() ?: return null
        if (port !in 1..65535) return null
        if (!isPlausibleHost(host)) return null

        return Target(host, port)
    }

    /**
     * A host name with nothing in it that could mean something else.
     *
     * Deliberately narrow: letters, digits, dots, dashes, underscores, and the
     * brackets and colons of an IPv6 literal. A control character or a space here
     * would be forwarded into a SOCKS request, and a name is not the place to
     * find out whether the far end is careful.
     */
    private fun isPlausibleHost(host: String): Boolean {
        if (host.isEmpty() || host.length > 255) return false
        return host.all { char ->
            char.isLetterOrDigit() || char == '.' || char == '-' || char == '_' ||
                char == ':' || char == '[' || char == ']'
        }
    }

    const val ESTABLISHED = "HTTP/1.1 200 Connection Established\r\n\r\n"

    fun refusal(reason: String): String =
        "HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nX-Wat-Reason: $reason\r\n\r\n"

    const val NOT_ALLOWED = "HTTP/1.1 405 Method Not Allowed\r\nContent-Length: 0\r\n\r\n"
}

package com.whatabrowser.wat.webview

/**
 * The SOCKS5 bytes, on their own so they can be checked without a socket.
 *
 * Tor speaks SOCKS5 and Android's WebView speaks only to HTTP proxies, so
 * something has to sit between them — [TorBridge] — and this is the half of that
 * which is pure arithmetic on bytes.
 *
 * The important detail is [connect] sending a **host name** rather than an
 * address. A browser that resolved the name itself would send a DNS query
 * straight out of the phone, in the clear, naming every site visited: the traffic
 * would be inside Tor and the lookups outside it. Handing tor the name is what
 * makes it do the resolving, inside the circuit.
 */
object Socks5 {

    const val VERSION = 5

    /** No authentication: the proxy is on this device and belongs to this app. */
    fun greeting(): ByteArray = byteArrayOf(VERSION.toByte(), 1, METHOD_NONE)

    fun greetingAccepted(reply: ByteArray): Boolean =
        reply.size >= 2 && reply[0].toInt() == VERSION && reply[1] == METHOD_NONE

    /**
     * `CONNECT host:port`, with the host as a name for tor to resolve.
     *
     * Returns null for anything that cannot be encoded — a name longer than a
     * byte can measure, or a port outside the range — rather than truncating,
     * which would connect somewhere other than where it was asked to.
     */
    fun connect(host: String, port: Int): ByteArray? {
        val name = host.toByteArray(Charsets.US_ASCII)
        if (name.isEmpty() || name.size > 255) return null
        if (port !in 1..65535) return null

        val out = ByteArray(4 + 1 + name.size + 2)
        out[0] = VERSION.toByte()
        out[1] = COMMAND_CONNECT
        out[2] = 0 // reserved
        out[3] = ADDRESS_NAME
        out[4] = name.size.toByte()
        name.copyInto(out, 5)
        out[out.size - 2] = ((port shr 8) and 0xFF).toByte()
        out[out.size - 1] = (port and 0xFF).toByte()
        return out
    }

    /** The reply's status byte, or [REPLY_MALFORMED] if it is not a reply at all. */
    fun status(reply: ByteArray): Int {
        if (reply.size < 4 || reply[0].toInt() != VERSION) return REPLY_MALFORMED
        return reply[1].toInt() and 0xFF
    }

    /**
     * How many more bytes follow the four-byte reply header, given the address
     * type byte and — for a name — the length byte after it.
     */
    fun addressLength(addressType: Int, firstByte: Int): Int = when (addressType) {
        ADDRESS_IPV4 -> 4 + 2
        ADDRESS_IPV6 -> 16 + 2
        ADDRESS_NAME.toInt() -> 1 + firstByte + 2
        else -> -1
    }

    /** What to put in the log, and nowhere else: this text never reaches a page. */
    fun describe(status: Int): String = when (status) {
        0 -> "connected"
        1 -> "general failure"
        2 -> "connection not allowed"
        3 -> "network unreachable"
        4 -> "host unreachable"
        5 -> "connection refused"
        6 -> "time to live expired"
        7 -> "command not supported"
        8 -> "address type not supported"
        REPLY_MALFORMED -> "not a SOCKS5 reply"
        else -> "unknown SOCKS5 status $status"
    }

    const val REPLY_OK = 0
    const val REPLY_MALFORMED = -1

    const val ADDRESS_IPV4 = 1
    const val ADDRESS_IPV6 = 4
    const val ADDRESS_NAME: Byte = 3

    private const val METHOD_NONE: Byte = 0
    private const val COMMAND_CONNECT: Byte = 1
}

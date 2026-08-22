package com.whatabrowser.wat.webview

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class Socks5Test {

    @Test
    fun `the greeting offers no authentication and checks the answer`() {
        assertArrayEquals(byteArrayOf(5, 1, 0), Socks5.greeting())
        assertTrue(Socks5.greetingAccepted(byteArrayOf(5, 0)))
        // A proxy demanding a password, a proxy speaking SOCKS4, a truncated read.
        assertFalse(Socks5.greetingAccepted(byteArrayOf(5, 2)))
        assertFalse(Socks5.greetingAccepted(byteArrayOf(4, 0)))
        assertFalse(Socks5.greetingAccepted(byteArrayOf(5)))
    }

    @Test
    fun `connect sends the host as a name, so tor does the resolving`() {
        val request = Socks5.connect("example.com", 443)!!
        assertEquals(5, request[0].toInt()) // version
        assertEquals(1, request[1].toInt()) // CONNECT
        assertEquals(0, request[2].toInt()) // reserved
        assertEquals(3, request[3].toInt()) // address type: name, not address
        assertEquals("example.com".length, request[4].toInt())
        assertEquals("example.com", String(request, 5, 11, Charsets.US_ASCII))
        // Port, big-endian: 443 is 0x01BB.
        assertEquals(0x01, request[request.size - 2].toInt())
        assertEquals(0xBB, request[request.size - 1].toInt() and 0xFF)
    }

    @Test
    fun `a port that does not fit is refused rather than wrapped`() {
        assertNull(Socks5.connect("example.com", 0))
        assertNull(Socks5.connect("example.com", 65536))
        assertNull(Socks5.connect("example.com", -1))
        assertEquals(0xFF, Socks5.connect("example.com", 65535)!!.last().toInt() and 0xFF)
    }

    @Test
    fun `a name too long to measure is refused rather than truncated`() {
        // Truncating would connect to a different host than the one asked for.
        assertNull(Socks5.connect("a".repeat(256), 443))
        assertNull(Socks5.connect("", 443))
        // 4 header bytes, 1 length byte, 255 of name, 2 of port.
        assertEquals(262, Socks5.connect("a".repeat(255), 443)!!.size)
    }

    @Test
    fun `a reply is read for its status, and nonsense is not a success`() {
        assertEquals(Socks5.REPLY_OK, Socks5.status(byteArrayOf(5, 0, 0, 1)))
        assertEquals(5, Socks5.status(byteArrayOf(5, 5, 0, 1)))
        assertEquals(Socks5.REPLY_MALFORMED, Socks5.status(byteArrayOf(4, 0, 0, 1)))
        assertEquals(Socks5.REPLY_MALFORMED, Socks5.status(byteArrayOf(5, 0)))
        assertEquals(Socks5.REPLY_MALFORMED, Socks5.status(ByteArray(0)))
    }

    @Test
    fun `the bound address after the reply header is measured by its type`() {
        assertEquals(6, Socks5.addressLength(Socks5.ADDRESS_IPV4, 0))
        assertEquals(18, Socks5.addressLength(Socks5.ADDRESS_IPV6, 0))
        assertEquals(14, Socks5.addressLength(Socks5.ADDRESS_NAME.toInt(), 11))
        assertEquals(-1, Socks5.addressLength(9, 0))
    }

    @Test
    fun `every status has something to say`() {
        for (status in 0..8) {
            assertTrue(Socks5.describe(status).isNotEmpty())
        }
        assertTrue(Socks5.describe(Socks5.REPLY_MALFORMED).contains("SOCKS5"))
        assertTrue(Socks5.describe(99).contains("99"))
    }
}

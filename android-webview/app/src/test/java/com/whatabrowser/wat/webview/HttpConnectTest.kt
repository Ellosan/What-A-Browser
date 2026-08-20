package com.whatabrowser.wat.webview

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The bridge listens on a local port that any app on the device can reach, so
 * the request line is untrusted input and these are the tests for that.
 */
class HttpConnectTest {

    @Test
    fun `a well-formed tunnel request is understood`() {
        val target = HttpConnect.parse("CONNECT example.com:443 HTTP/1.1")!!
        assertEquals("example.com", target.host)
        assertEquals(443, target.port)
        // Some clients lowercase the verb.
        assertNotNull(HttpConnect.parse("connect example.com:443 HTTP/1.1"))
    }

    @Test
    fun `an IPv6 literal keeps its colons`() {
        val target = HttpConnect.parse("CONNECT [2001:db8::1]:8443 HTTP/1.1")!!
        assertEquals("[2001:db8::1]", target.host)
        assertEquals(8443, target.port)
        assertNull(HttpConnect.parse("CONNECT [2001:db8::1:8443 HTTP/1.1"))
    }

    @Test
    fun `anything other than a tunnel is refused`() {
        // The bridge forwards tunnels. It must never be talked into fetching a
        // URL for someone.
        assertNull(HttpConnect.parse("GET http://example.com/ HTTP/1.1"))
        assertNull(HttpConnect.parse("POST / HTTP/1.1"))
        assertNull(HttpConnect.parse(""))
        assertNull(HttpConnect.parse("CONNECT"))
    }

    @Test
    fun `a missing or impossible port is refused`() {
        assertNull(HttpConnect.parse("CONNECT example.com HTTP/1.1"))
        assertNull(HttpConnect.parse("CONNECT example.com: HTTP/1.1"))
        assertNull(HttpConnect.parse("CONNECT example.com:0 HTTP/1.1"))
        assertNull(HttpConnect.parse("CONNECT example.com:99999 HTTP/1.1"))
        assertNull(HttpConnect.parse("CONNECT example.com:https HTTP/1.1"))
        assertNull(HttpConnect.parse("CONNECT :443 HTTP/1.1"))
    }

    @Test
    fun `a host with anything odd in it is refused`() {
        val nul = 0.toChar()
        val newline = 10.toChar()
        assertNull(HttpConnect.parse("CONNECT exa${nul}mple.com:443 HTTP/1.1"))
        assertNull(HttpConnect.parse("CONNECT exa${newline}mple.com:443 HTTP/1.1"))
        assertNull(HttpConnect.parse("CONNECT exam ple.com:443 HTTP/1.1"))
        assertNull(HttpConnect.parse("CONNECT ex@mple.com:443 HTTP/1.1"))
        assertNull(HttpConnect.parse("CONNECT " + "a".repeat(300) + ":443 HTTP/1.1"))
    }

    @Test
    fun `an oversized line is dropped before it is parsed`() {
        val huge = "CONNECT " + "a".repeat(HttpConnect.MAX_LINE) + ":443"
        assertNull(HttpConnect.parse(huge))
    }

    @Test
    fun `the replies are complete HTTP messages`() {
        assertTrue(HttpConnect.ESTABLISHED.startsWith("HTTP/1.1 200"))
        assertTrue(HttpConnect.ESTABLISHED.endsWith("\r\n\r\n"))
        assertTrue(HttpConnect.NOT_ALLOWED.endsWith("\r\n\r\n"))
        assertTrue(HttpConnect.refusal("no tor").endsWith("\r\n\r\n"))
    }
}

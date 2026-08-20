package com.whatabrowser.wat.webview

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * The scheme policy is a security boundary, so it is tested rather than trusted.
 */
class UrlResolverTest {

    private val search = "https://duckduckgo.com/?q={}"

    @Test
    fun `only http and https are rendered`() {
        assertEquals(UrlResolver.Decision.RENDER, UrlResolver.decide("https://example.com/"))
        assertEquals(UrlResolver.Decision.RENDER, UrlResolver.decide("http://example.com/"))
        // Case is not a way around it.
        assertEquals(UrlResolver.Decision.RENDER, UrlResolver.decide("HTTPS://example.com/"))
    }

    @Test
    fun `the dangerous schemes are refused`() {
        for (url in listOf(
            "javascript:alert(document.cookie)",
            "JaVaScRiPt:alert(1)",
            "file:///data/data/com.whatabrowser.wat.webview/databases",
            "content://com.android.contacts/contacts",
            "data:text/html,<script>alert(1)</script>",
            "intent://scan/#Intent;scheme=zxing;end",
            "about:blank",
        )) {
            assertEquals("$url must not render", UrlResolver.Decision.REFUSE, UrlResolver.decide(url))
        }
    }

    @Test
    fun `mailto and friends go to the system`() {
        assertEquals(UrlResolver.Decision.ASK_SYSTEM, UrlResolver.decide("mailto:someone@example.com"))
        assertEquals(UrlResolver.Decision.ASK_SYSTEM, UrlResolver.decide("tel:+441234567890"))
    }

    @Test
    fun `a bare host is upgraded to https, never http`() {
        assertEquals("https://example.com", UrlResolver.resolve("example.com", search))
        assertEquals("https://example.com/path", UrlResolver.resolve("example.com/path", search))
        assertEquals("https://sub.example.co.uk", UrlResolver.resolve("sub.example.co.uk", search))
    }

    @Test
    fun `an explicit scheme is kept`() {
        assertEquals("http://example.com/", UrlResolver.resolve("http://example.com/", search))
        assertEquals("https://example.com/", UrlResolver.resolve("https://example.com/", search))
    }

    @Test
    fun `a dangerous scheme typed into the address bar becomes a search`() {
        // Not loaded, not run: searched for. This is the address-bar half of the
        // same boundary `decide` enforces for links.
        val resolved = UrlResolver.resolve("javascript:alert(1)", search)
        assertEquals("https://duckduckgo.com/?q=javascript%3Aalert%281%29", resolved)
    }

    @Test
    fun `anything else is a search`() {
        assertEquals("https://duckduckgo.com/?q=how+tall+is+everest", UrlResolver.resolve("how tall is everest", search))
        // One word with no dot is a search, not a hostname lookup.
        assertEquals("https://duckduckgo.com/?q=example", UrlResolver.resolve("example", search))
        // A trailing dot is not a host either.
        assertEquals("https://duckduckgo.com/?q=example.", UrlResolver.resolve("example.", search))
    }

    @Test
    fun `localhost is a host even without a dot`() {
        assertEquals("https://localhost", UrlResolver.resolve("localhost", search))
        assertEquals("https://localhost:8080", UrlResolver.resolve("localhost:8080", search))
    }

    @Test
    fun `empty input does nothing`() {
        assertEquals("", UrlResolver.resolve("", search))
        assertEquals("", UrlResolver.resolve("   ", search))
    }
}

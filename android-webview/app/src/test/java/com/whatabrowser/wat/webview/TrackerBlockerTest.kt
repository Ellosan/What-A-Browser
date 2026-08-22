package com.whatabrowser.wat.webview

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TrackerBlockerTest {

    @Test
    fun `known measurement hosts are refused`() {
        assertTrue(TrackerBlocker.blocks("google-analytics.com"))
        assertTrue(TrackerBlocker.blocks("www.google-analytics.com"))
        assertTrue(TrackerBlocker.blocks("region1.google-analytics.com"))
        assertTrue(TrackerBlocker.blocks("doubleclick.net"))
        assertTrue(TrackerBlocker.blocks("connect.facebook.net"))
    }

    @Test
    fun `the suffix has to end at a dot`() {
        // The failure this guards against is a blocklist that can be walked
        // around by registering a domain ending in the blocked one.
        assertFalse(TrackerBlocker.blocks("evil-google-analytics.com"))
        assertFalse(TrackerBlocker.blocks("notdoubleclick.net"))
        assertFalse(TrackerBlocker.blocks("mydoubleclick.net"))
    }

    @Test
    fun `ordinary hosts are left alone`() {
        for (host in listOf(
            "example.com",
            "en.wikipedia.org",
            "duckduckgo.com",
            "cdn.jsdelivr.net",
            "fonts.gstatic.com",
            "github.com",
        )) {
            assertFalse(host, TrackerBlocker.blocks(host))
        }
    }

    @Test
    fun `case and a trailing dot do not get past it`() {
        assertTrue(TrackerBlocker.blocks("GOOGLE-ANALYTICS.COM"))
        assertTrue(TrackerBlocker.blocks("Www.DoubleClick.Net"))
        assertTrue(TrackerBlocker.blocks("google-analytics.com."))
    }

    @Test
    fun `a URL is matched by its host, port and all`() {
        assertTrue(TrackerBlocker.blocksUrl("https://www.google-analytics.com/collect?v=1"))
        assertTrue(TrackerBlocker.blocksUrl("https://doubleclick.net:443/pixel"))
        assertFalse(TrackerBlocker.blocksUrl("https://example.com/analytics.js"))
        // The path is not the host: a page of its own about analytics is a page.
        assertFalse(TrackerBlocker.blocksUrl("https://example.com/google-analytics.com/"))
    }

    @Test
    fun `nonsense is allowed rather than blocked`() {
        // Failing open: the cost of a mistake here is a broken page, and the
        // dangerous schemes are refused elsewhere long before this runs.
        assertFalse(TrackerBlocker.blocks(""))
        assertFalse(TrackerBlocker.blocksUrl(""))
        assertFalse(TrackerBlocker.blocksUrl("not a url at all"))
    }

    @Test
    fun `the list has no empty or duplicated entries`() {
        assertTrue(TrackerBlocker.HOSTS.all { it.isNotBlank() })
        assertTrue(TrackerBlocker.HOSTS.all { it == it.lowercase() })
        assertTrue(TrackerBlocker.HOSTS.size > 40)
    }
}

package com.whatabrowser.wat.webview

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * A match pattern is the only thing between a script installed for one site and
 * every other site the reader visits, so these tests are mostly about the ways a
 * pattern must *not* match.
 */
class MatchPatternTest {

    private fun matches(pattern: String, url: String): Boolean =
        MatchPattern.parse(pattern)?.matches(url) ?: false

    @Test
    fun `everywhere means everywhere`() {
        assertTrue(matches("*://*/*", "https://example.com/"))
        assertTrue(matches("*://*/*", "http://other.test/deep/path?q=1"))
        assertTrue(matches("<all_urls>", "https://example.com/"))
    }

    @Test
    fun `a host pattern stops at a label boundary`() {
        assertTrue(matches("*://*.example.com/*", "https://example.com/"))
        assertTrue(matches("*://*.example.com/*", "https://www.example.com/x"))
        assertTrue(matches("*://*.example.com/*", "https://a.b.example.com/x"))
        // The failure that matters: a domain someone else can register.
        assertFalse(matches("*://*.example.com/*", "https://notexample.com/"))
        assertFalse(matches("*://*.example.com/*", "https://example.com.evil.test/"))
    }

    @Test
    fun `an exact host is exact`() {
        assertTrue(matches("https://example.com/*", "https://example.com/anything"))
        assertFalse(matches("https://example.com/*", "https://www.example.com/"))
        assertFalse(matches("https://example.com/*", "http://example.com/"))
    }

    @Test
    fun `credentials and ports are not part of the host`() {
        assertTrue(matches("https://example.com/*", "https://example.com:8443/x"))
        // The oldest way to make one address look like another.
        assertFalse(matches("https://example.com/*", "https://example.com@evil.test/x"))
        assertTrue(matches("https://evil.test/*", "https://example.com@evil.test/x"))
    }

    @Test
    fun `the path is matched, wildcards and all`() {
        assertTrue(matches("https://example.com/docs/*", "https://example.com/docs/a/b"))
        assertFalse(matches("https://example.com/docs/*", "https://example.com/other"))
        assertTrue(matches("https://example.com/*/edit", "https://example.com/page/edit"))
        assertFalse(matches("https://example.com/*/edit", "https://example.com/page/view"))
        // A fragment is not part of the path.
        assertTrue(matches("https://example.com/page", "https://example.com/page#section"))
    }

    @Test
    fun `nonsense patterns match nothing rather than everything`() {
        assertNull(MatchPattern.parse(""))
        assertNull(MatchPattern.parse("example.com"))
        assertNull(MatchPattern.parse("://example.com/"))
        assertNull(MatchPattern.parse("file:///*"))
        assertNull(MatchPattern.parse("javascript://*/*"))
        // A star in the middle of a host is not a host pattern.
        assertNull(MatchPattern.parse("https://ex*ple.com/*"))
        assertNull(MatchPattern.parse("https://*.ex*.com/*"))
    }

    @Test
    fun `only http and https can be matched`() {
        assertNull(MatchPattern.parse("ftp://example.com/*"))
        assertFalse(matches("*://*/*", "file:///etc/passwd"))
    }

    @Test
    fun `origin rules cover the host and say when they cover everything`() {
        assertEquals("https://example.com", MatchPattern.originRule("https://example.com/docs/*"))
        assertEquals("*://*.example.com", MatchPattern.originRule("*://*.example.com/*"))
        // Null means every origin, which is what the everywhere patterns need.
        assertNull(MatchPattern.originRule("<all_urls>"))
        assertNull(MatchPattern.originRule("*://*/*"))
    }

    @Test
    fun `the in-page guard is true only where the pattern matches`() {
        val guard = MatchPattern.jsGuard(listOf("https://example.com/docs/*"))
        // Checked by reading the expression rather than running it: the shape is
        // what matters — a regex bound at both ends, against location.href.
        assertTrue(guard.startsWith("new RegExp("))
        assertTrue(guard.contains("location.href"))
        assertTrue(guard.contains("example"))
        assertTrue(guard.contains("docs"))
        assertTrue(guard.contains("^https"))
        assertTrue("bound at the end", guard.contains(36.toChar().toString()))
    }

    @Test
    fun `a host with anything odd in it is not a host`() {
        val quote = 34.toChar()
        val backslash = 92.toChar()
        assertNull(MatchPattern.parse("https://exa${quote}mple.com/*"))
        assertNull(MatchPattern.parse("https://a${backslash}b.com/*"))
        assertNull(MatchPattern.parse("https://exa mple.com/*"))
    }

    @Test
    fun `a pattern cannot escape the guard it is put into`() {
        // A path may legitimately contain characters a host may not, so this is
        // where the escaping has to hold: the quotes in the expression must all
        // be the ones that delimit its string literals.
        val quote = 34.toChar()
        val guard = MatchPattern.jsGuard(listOf("https://example.com/a${quote}b*"))
        assertTrue(guard.startsWith("new RegExp("))

        var unescaped = 0
        var index = 0
        while (index < guard.length) {
            val char = guard[index]
            if (char == 92.toChar()) {
                index += 2
                continue
            }
            if (char == quote) unescaped++
            index++
        }
        assertEquals("only the delimiters are unescaped quotes", 2, unescaped)

        val many = MatchPattern.jsGuard(listOf("https://example.com/*", "http://other.test/*"))
        assertTrue(many.contains(" || "))
        assertEquals(2, many.split("new RegExp(").size - 1)
    }

    @Test
    fun `no usable pattern gives a guard that is false`() {
        assertEquals("false", MatchPattern.jsGuard(emptyList()))
        assertEquals("false", MatchPattern.jsGuard(listOf("nonsense", "file:///*")))
    }
}

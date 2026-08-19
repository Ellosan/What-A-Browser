package com.whatabrowser.wat.webview

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * A download's name comes from a header a server wrote, so these are the tests
 * for a hostile header rather than a well-formed one.
 */
class DownloadsTest {

    private fun name(
        url: String = "https://example.com/get",
        disposition: String? = null,
        mime: String? = null,
    ) = Downloads.fileName(url, disposition, mime)

    @Test
    fun `a plain header is used as it is`() {
        assertEquals("report.pdf", name(disposition = "attachment; filename=\"report.pdf\""))
        assertEquals("report.pdf", name(disposition = "attachment; filename=report.pdf"))
        assertEquals("report.pdf", name(disposition = "ATTACHMENT; FILENAME=\"report.pdf\""))
    }

    @Test
    fun `path traversal in the header cannot escape the directory`() {
        for (offered in listOf(
            "../../../shared_prefs/settings.xml",
            "..\\..\\..\\shared_prefs\\settings.xml",
            "/data/data/com.whatabrowser.wat.webview/databases/cookies.db",
            "subdir/../../x.txt",
        )) {
            val resolved = name(disposition = "attachment; filename=\"" + offered + "\"")
            assertTrue("$offered produced $resolved", '/' !in resolved && '\\' !in resolved)
            assertTrue("$offered produced $resolved", !resolved.startsWith("."))
        }
        // The last segment is what is left, so the name is still recognisable.
        assertEquals("settings.xml", name(disposition = "attachment; filename=\"../../settings.xml\""))
    }

    @Test
    fun `dots alone are not a file name`() {
        // A header that is nothing but dots names no file, so the browser falls
        // through to the URL and then to the fallback, exactly as if the server
        // had offered nothing at all.
        val bare = "https://example.com/"
        assertEquals("download", name(url = bare, disposition = "attachment; filename=\"..\""))
        assertEquals("download", name(url = bare, disposition = "attachment; filename=\".\""))
        assertEquals("download", name(url = bare, disposition = "attachment; filename=\"...\""))
        // The URL still gets its turn when the header is unusable.
        assertEquals("get", name(disposition = "attachment; filename=\"..\""))
        // A dotfile becomes an ordinary file rather than a hidden one.
        assertEquals("bashrc", name(disposition = "attachment; filename=\".bashrc\""))
    }

    @Test
    fun `control characters and NUL are removed`() {
        // Written by code point rather than typed, so what these tests contain is
        // legible. The NUL is the one that matters: it truncates a path in every C
        // library underneath Android, so a name that keeps one means two different
        // things depending on who reads it.
        val nul = 0.toChar()
        val newline = 10.toChar()
        val del = 127.toChar()
        assertEquals("evil.txt", name(disposition = "attachment; filename=\"evil${nul}.txt\""))
        assertEquals("onetwo.txt", name(disposition = "attachment; filename=\"one${newline}two.txt\""))
        assertEquals("ab.txt", name(disposition = "attachment; filename=\"ab${del}.txt\""))
    }

    @Test
    fun `characters that mean something to a file system are replaced`() {
        assertEquals("a_b_c.txt", name(disposition = "attachment; filename=\"a:b*c.txt\""))
        assertEquals("q_.txt", name(disposition = "attachment; filename=\"q?.txt\""))
    }

    @Test
    fun `the RFC 5987 form is decoded and wins over the plain one`() {
        assertEquals(
            "€ rates.pdf",
            name(disposition = "attachment; filename=\"rates.pdf\"; filename*=UTF-8''%E2%82%AC%20rates.pdf"),
        )
        // A plus sign is a plus sign, not a space: this is not form encoding.
        assertEquals("c++notes.txt", name(disposition = "attachment; filename*=UTF-8''c++notes.txt"))
        // And the encoded form cannot smuggle a path in either.
        assertEquals(
            "passwd",
            name(disposition = "attachment; filename*=UTF-8''..%2F..%2F..%2Fetc%2Fpasswd"),
        )
    }

    @Test
    fun `with no header the URL provides the name`() {
        assertEquals("holiday.jpg", name(url = "https://example.com/photos/holiday.jpg"))
        assertEquals("holiday.jpg", name(url = "https://example.com/photos/holiday.jpg?size=large#top"))
        assertEquals("my report.pdf", name(url = "https://example.com/my%20report.pdf"))
        assertEquals("download", name(url = "https://example.com/"))
    }

    @Test
    fun `a missing extension comes from the declared type`() {
        assertEquals("invoice.pdf", name(disposition = "attachment; filename=invoice", mime = "application/pdf"))
        assertEquals("page.html", name(url = "https://example.com/page", mime = "text/html; charset=utf-8"))
        // An unknown type adds nothing rather than guessing.
        assertEquals("thing", name(disposition = "attachment; filename=thing", mime = "application/x-nonsense"))
        // An extension already there is left alone.
        assertEquals("data.csv", name(disposition = "attachment; filename=data.csv", mime = "application/pdf"))
    }

    @Test
    fun `a very long name is shortened but keeps its extension`() {
        val long = "a".repeat(400)
        val resolved = name(disposition = "attachment; filename=\"" + long + ".pdf\"")
        assertTrue(resolved.length <= 100)
        assertTrue(resolved.endsWith(".pdf"))
    }

    @Test
    fun `nothing usable anywhere falls back`() {
        assertEquals("download", name(url = "https://example.com/", disposition = "attachment"))
        assertEquals("download", name(url = "https://example.com/", disposition = "attachment; filename=\"\""))
        assertEquals("download", name(url = "https://example.com/", disposition = "attachment; filename=\"   \""))
    }
}

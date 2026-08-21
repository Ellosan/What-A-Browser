package com.whatabrowser.wat.webview

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class UserScriptTest {

    private val header = """
        // ==UserScript==
        // @name         Adv-Microslop
        // @namespace    adv-microslop@ellosan
        // @version      1.0.6
        // @description  Replaces some words with other words.
        // @author       Ellosan
        // @match        *://*/*
        // @grant        GM_setValue
        // @grant        GM_getValue
        // @grant        GM_registerMenuCommand
        // @grant        GM_addStyle
        // @run-at       document-end
        // @license      MIT
        // ==/UserScript==
        (function () { 'use strict'; })();
    """.trimIndent()

    @Test
    fun `the metadata block is read`() {
        val script = UserScript.parse(header)!!
        assertEquals("Adv-Microslop", script.name)
        assertEquals("1.0.6", script.version)
        assertEquals("Replaces some words with other words.", script.description)
        assertEquals(listOf("*://*/*"), script.matches)
        assertFalse(script.runAtStart)
        assertEquals(4, script.grants.size)
    }

    @Test
    fun `a script that grants what this browser has asks for nothing missing`() {
        // The four this one wants are all in the shim, so nothing is reported.
        assertTrue(UserScript.parse(header)!!.unsupportedGrants().isEmpty())
        val greedy = header.replace("// @grant        GM_addStyle", "// @grant        GM_xmlhttpRequest")
        assertEquals(listOf("GM_xmlhttpRequest"), UserScript.parse(greedy)!!.unsupportedGrants())
    }

    @Test
    fun `document-start is noticed`() {
        val early = header.replace("document-end", "document-start")
        assertTrue(UserScript.parse(early)!!.runAtStart)
    }

    @Test
    fun `include is read as well as match`() {
        val old = header.replace("// @match        *://*/*", "// @include      https://example.com/*")
        val script = UserScript.parse(old)!!
        assertEquals(listOf("https://example.com/*"), script.matches)
        assertTrue(script.appliesTo("https://example.com/page"))
        assertFalse(script.appliesTo("https://other.test/"))
    }

    @Test
    fun `exclude wins over match`() {
        val fussy = header.replace(
            "// @match        *://*/*",
            "// @match        *://*/*\n// @exclude      https://bank.example/*",
        )
        val script = UserScript.parse(fussy)!!
        assertTrue(script.appliesTo("https://example.com/"))
        assertFalse(script.appliesTo("https://bank.example/accounts"))
    }

    @Test
    fun `a script with no usable match runs nowhere`() {
        val vague = header.replace("// @match        *://*/*", "// @match        nonsense")
        assertFalse(UserScript.parse(vague)!!.appliesTo("https://example.com/"))
        val none = header.replace("// @match        *://*/*", "")
        assertFalse(UserScript.parse(none)!!.appliesTo("https://example.com/"))
    }

    @Test
    fun `a script never runs on anything the browser would refuse to load`() {
        val script = UserScript.parse(header)!!
        assertFalse(script.appliesTo("file:///etc/passwd"))
        assertFalse(script.appliesTo("javascript:alert(1)"))
        assertFalse(script.appliesTo("about:blank"))
        assertFalse(script.appliesTo("content://media/external/images"))
    }

    @Test
    fun `something that is not a userscript is refused`() {
        assertNull(UserScript.parse("alert('hello')"))
        assertNull(UserScript.parse(""))
        // A block with no name is not installable: there would be nothing to
        // show in the list, and nothing to turn off.
        assertNull(UserScript.parse("// ==UserScript==\n// @match *://*/*\n// ==/UserScript==\n"))
        // Absurdly large input is refused rather than held in memory.
        assertNull(UserScript.parse("x".repeat(UserScript.MAX_SOURCE + 1)))
    }
}

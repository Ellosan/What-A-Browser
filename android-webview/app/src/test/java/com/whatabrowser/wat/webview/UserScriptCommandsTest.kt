package com.whatabrowser.wat.webview

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The reply from a page is text this browser did not write, so the tests here
 * are mostly about answers that are wrong on purpose.
 */
class UserScriptCommandsTest {

    private val field = '\u0000'
    private val record = '\u0001'

    /** What `evaluateJavascript` does to a string before handing it over. */
    private fun asReply(value: String): String {
        val out = StringBuilder("\"")
        for (char in value) {
            when {
                char == '"' -> out.append("\\\"")
                char == '\\' -> out.append("\\\\")
                char.code < 0x20 -> out.append("\\u%04x".format(char.code))
                else -> out.append(char)
            }
        }
        return out.append('"').toString()
    }

    @Test
    fun `reads the commands a page registered`() {
        val page = listOf(
            "0${field}Adv-Microslop${field}Adv-Microslop Settings",
            "1${field}Another${field}Do the thing",
        ).joinToString(record.toString())

        val commands = UserScriptCommands.parse(asReply(page))

        assertEquals(2, commands.size)
        assertEquals(0, commands[0].index)
        assertEquals("Adv-Microslop", commands[0].script)
        assertEquals("Adv-Microslop Settings", commands[0].caption)
        assertEquals(1, commands[1].index)
        assertEquals("Do the thing", commands[1].caption)
    }

    @Test
    fun `a page with no scripts has no commands`() {
        assertEquals(0, UserScriptCommands.parse(asReply("")).size)
        assertEquals(0, UserScriptCommands.parse("null").size)
        assertEquals(0, UserScriptCommands.parse(null).size)
    }

    @Test
    fun `a malformed record is dropped, not guessed at`() {
        val page = listOf(
            "not a number${field}Script${field}Fine words",
            "1${field}only two fields",
            "2${field}Script${field}   ",
            "3${field}Script${field}Real",
        ).joinToString(record.toString())

        val commands = UserScriptCommands.parse(asReply(page))

        assertEquals(1, commands.size)
        assertEquals("Real", commands[0].caption)
        assertEquals(3, commands[0].index)
    }

    @Test
    fun `an answer that is not a string is not a command list`() {
        assertNull(UserScriptCommands.decode("undefined"))
        assertNull(UserScriptCommands.decode("[1,2,3]"))
        assertNull(UserScriptCommands.decode("\"unterminated"))
        assertNull(UserScriptCommands.decode("\"two\" \"literals\""))
        assertNull(UserScriptCommands.decode("\"\\q\""))
        assertNull(UserScriptCommands.decode("\"\\u00\""))
    }

    @Test
    fun `every escape a reply can carry comes back out`() {
        val awkward = "quote\" slash/ back\\slash tab\t newline\n emoji 🦁 accent é"
        assertEquals(awkward, UserScriptCommands.decode(asReply(awkward)))
    }

    @Test
    fun `a caption cannot smuggle in a separator`() {
        // The page strips both separators before joining, so a caption that
        // contains one arrives with spaces in their place. The parser is told the
        // same thing by the field count: a record with four fields is dropped.
        val page = "0${field}Script${field}Settings${field}extra"
        assertEquals(0, UserScriptCommands.parse(asReply(page)).size)
    }

    @Test
    fun `the page-side script strips the separators it joins on`() {
        assertTrue(UserScriptCommands.LIST.contains("replace(strip, ' ')"))
        assertTrue(UserScriptCommands.LIST.contains("__watUserscriptMenu"))
        assertTrue(UserScriptCommands.invoke(7).contains("menu[7]"))
    }
}

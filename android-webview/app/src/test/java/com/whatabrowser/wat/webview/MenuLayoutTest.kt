package com.whatabrowser.wat.webview

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MenuLayoutTest {

    @Test
    fun `by default every item is there, in order`() {
        val layout = MenuLayout.default()
        assertEquals(MenuLayout.ALL, layout.visible())
    }

    @Test
    fun `hiding and showing an item survives a round trip`() {
        val layout = MenuLayout.default()
        layout.toggle("downloads")
        assertFalse(layout.isVisible("downloads"))

        val reloaded = MenuLayout.parse(layout.serialize())
        assertFalse(reloaded.isVisible("downloads"))
        // Hidden, not gone: it still has a place in the customise list.
        assertTrue(reloaded.items().any { it.id == "downloads" })

        reloaded.toggle("downloads")
        assertTrue(reloaded.isVisible("downloads"))
    }

    @Test
    fun `the way back is never removable`() {
        val layout = MenuLayout.default()
        for (id in MenuLayout.ALWAYS) {
            layout.toggle(id)
            assertTrue("$id must stay", layout.isVisible(id))
        }
        // Not even by editing the stored string by hand.
        assertTrue(MenuLayout.parse("-settings,-customize").isVisible("settings"))
    }

    @Test
    fun `moving an item changes the order and stops at the ends`() {
        val layout = MenuLayout.default()
        val second = MenuLayout.ALL[1]
        layout.moveUp(second)
        assertEquals(second, layout.visible().first())

        layout.moveUp(second)
        assertEquals(second, layout.visible().first())

        layout.moveDown(second)
        assertEquals(second, layout.visible()[1])
    }

    @Test
    fun `an item this version does not know about is dropped`() {
        val layout = MenuLayout.parse("new_tab,teleport,settings")
        assertFalse(layout.items().any { it.id == "teleport" })
        assertEquals("new_tab", layout.visible().first())
    }

    @Test
    fun `an item the stored order predates is added rather than lost`() {
        // What an upgrade looks like: a menu saved before these existed.
        val layout = MenuLayout.parse("new_tab,settings")
        assertTrue(layout.isVisible("private_lion"))
        assertEquals(MenuLayout.ALL.size, layout.items().size)
        // And the reader's own order is kept in front of the new arrivals.
        assertEquals(listOf("new_tab", "settings"), layout.visible().take(2))
    }

    @Test
    fun `a repeated id is not duplicated`() {
        val layout = MenuLayout.parse("find,find,find")
        assertEquals(1, layout.items().count { it.id == "find" })
    }

    @Test
    fun `reset puts everything back`() {
        val layout = MenuLayout.default()
        layout.toggle("share")
        layout.moveDown("new_tab")
        layout.reset()
        assertEquals(MenuLayout.ALL, layout.visible())
    }
}

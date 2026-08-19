package com.whatabrowser.wat.webview

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Test

/** Tab order and selection: the fiddly part of tabs, without any views. */
class TabListTest {

    private fun urls(list: TabList) = list.all().map { it.url }

    @Test
    fun `a new tab from a page opens next to it, not at the end`() {
        val list = TabList()
        val first = list.open("https://a/")!!
        list.open("https://b/")
        list.select(first.id)
        list.open("https://link/")
        assertEquals(listOf("https://a/", "https://link/", "https://b/"), urls(list))
    }

    @Test
    fun `a background tab does not steal the selection`() {
        val list = TabList()
        val first = list.open("https://a/")!!
        val second = list.open("https://b/", foreground = false)!!
        assertEquals(first.id, list.currentId)
        assertEquals("https://b/", list.byId(second.id)?.url)
    }

    @Test
    fun `closing the current tab moves right, then left`() {
        val list = TabList()
        val a = list.open("https://a/")!!
        val b = list.open("https://b/", foreground = false, after = a.id)!!
        val c = list.open("https://c/", foreground = false, after = b.id)!!

        list.select(b.id)
        assertEquals(c.id, list.close(b.id)?.id)

        list.select(c.id)
        // Nothing to the right of c any more, so the selection falls back left.
        assertEquals(a.id, list.close(c.id)?.id)
    }

    @Test
    fun `closing a tab that is not in front leaves the selection alone`() {
        val list = TabList()
        val a = list.open("https://a/")!!
        val b = list.open("https://b/", foreground = false)!!
        list.select(a.id)
        assertEquals(a.id, list.close(b.id)?.id)
        assertEquals(1, list.count)
    }

    @Test
    fun `closing the last tab leaves nothing and says so`() {
        val list = TabList()
        val only = list.open("https://a/")!!
        assertNull(list.close(only.id))
        assertEquals(0, list.count)
        assertNull(list.currentId)
    }

    @Test
    fun `the tab limit is reported rather than exceeded`() {
        val list = TabList(maxTabs = 3)
        assertNotNull(list.open("https://a/"))
        assertNotNull(list.open("https://b/"))
        assertNotNull(list.open("https://c/"))
        assertNull(list.open("https://d/"))
        assertEquals(3, list.count)
    }

    @Test
    fun `the least recently used tab is the one to give up`() {
        val list = TabList()
        val a = list.open("https://a/")!!
        val b = list.open("https://b/")!!
        val c = list.open("https://c/")!!
        list.select(a.id)
        list.select(c.id)

        // b has gone longest without being looked at; c is in front and is never
        // offered up.
        assertEquals(b.id, list.leastRecentlyUsed(listOf(a.id, b.id, c.id)))
        // Only tabs that actually hold a view are candidates.
        assertEquals(a.id, list.leastRecentlyUsed(listOf(a.id, c.id)))
        assertNull(list.leastRecentlyUsed(listOf(c.id)))
    }

    @Test
    fun `a never-selected background tab counts as least recently used`() {
        val list = TabList()
        val a = list.open("https://a/")!!
        val b = list.open("https://b/", foreground = false)!!
        assertEquals(b.id, list.leastRecentlyUsed(listOf(a.id, b.id)))
    }

    @Test
    fun `a restored session keeps its order, its selection and its ids`() {
        val list = TabList()
        list.restore(
            listOf(TabList.Tab(4, "https://a/", "A"), TabList.Tab(9, "https://b/", "B")),
            selected = 9,
        )
        assertEquals(listOf("https://a/", "https://b/"), urls(list))
        assertEquals(9, list.currentId)
        // A new tab must not reuse an id that came back from the saved session.
        assertEquals(10, list.open("https://c/")!!.id)
    }

    @Test
    fun `a tab shows its title, or its address until it has one`() {
        val tab = TabList.Tab(1, "https://example.com/")
        assertEquals("https://example.com/", tab.label)
        tab.title = "Example"
        assertEquals("Example", tab.label)
    }
}

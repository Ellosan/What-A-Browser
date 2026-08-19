package com.whatabrowser.wat.webview

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class SearchEnginesTest {

    @Test
    fun `every offered engine is usable and secure`() {
        for (engine in SearchEngines.ALL) {
            assertTrue(engine.name, SearchEngines.isUsableTemplate(engine.template))
            assertTrue(engine.name, engine.home.startsWith("https://"))
            // The template has to survive the resolver, or a typed search would
            // produce something the browser then refuses to load.
            val resolved = UrlResolver.resolve("two words", engine.template)
            assertEquals(UrlResolver.Decision.RENDER, UrlResolver.decide(resolved))
        }
    }

    @Test
    fun `an unknown or missing name falls back to the default`() {
        assertEquals(SearchEngines.DEFAULT.name, SearchEngines.byName(null).name)
        assertEquals(SearchEngines.DEFAULT.name, SearchEngines.byName("Ask Jeeves").name)
        assertEquals("Google", SearchEngines.byName("Google").name)
    }

    @Test
    fun `a template that would downgrade or drop the terms is rejected`() {
        assertFalse(SearchEngines.isUsableTemplate("http://example.com/?q={}"))
        assertFalse(SearchEngines.isUsableTemplate("https://example.com/"))
        assertFalse(SearchEngines.isUsableTemplate(""))
    }
}

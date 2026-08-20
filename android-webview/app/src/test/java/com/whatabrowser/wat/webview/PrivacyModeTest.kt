package com.whatabrowser.wat.webview

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PrivacyModeTest {

    @Test
    fun `private modes write nothing down`() {
        for (mode in listOf(PrivacyMode.CAT, PrivacyMode.LION)) {
            assertTrue(mode.isPrivate)
            assertFalse(mode.recordsHistory)
            assertFalse(mode.suggestsFromHistory)
        }
        assertTrue(PrivacyMode.NORMAL.recordsHistory)
        assertFalse(PrivacyMode.NORMAL.isPrivate)
    }

    @Test
    fun `only the lion uses Tor`() {
        assertTrue(PrivacyMode.LION.usesTor)
        assertFalse(PrivacyMode.CAT.usesTor)
        assertFalse(PrivacyMode.NORMAL.usesTor)
    }

    @Test
    fun `every mode has a storage jar of its own`() {
        val suffixes = PrivacyMode.entries.map { it.storageSuffix }
        assertEquals(suffixes.size, suffixes.toSet().size)
    }

    @Test
    fun `an unknown mode is the ordinary one`() {
        assertEquals(PrivacyMode.NORMAL, PrivacyMode.of(null))
        assertEquals(PrivacyMode.NORMAL, PrivacyMode.of("SOMETHING_ELSE"))
        assertEquals(PrivacyMode.LION, PrivacyMode.of("LION"))
    }
}

package com.whatabrowser.wat.webview

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class UserAgentsTest {

    private val android = "Mozilla/5.0 (Linux; Android 13; Pixel 6 Build/TQ3A) AppleWebKit/537.36 " +
        "(KHTML, like Gecko) Chrome/119.0.6045.163 Mobile Safari/537.36"

    @Test
    fun `the desktop string keeps the real engine version`() {
        assertTrue(UserAgents.desktop(android).contains("Chrome/119.0.6045.163"))
    }

    @Test
    fun `it says desktop and stops saying Android`() {
        val desktop = UserAgents.desktop(android)
        assertTrue(desktop.contains("X11; Linux x86_64"))
        assertFalse(desktop.contains("Mobile"))
        assertFalse(desktop.contains("Android"))
        // Nothing identifying: a unique user agent is a tracking signal.
        assertFalse(desktop.contains("WAT"))
    }

    @Test
    fun `an unfamiliar default still produces something usable`() {
        val desktop = UserAgents.desktop("Mozilla/5.0 (something else entirely)")
        assertTrue(desktop.startsWith("Mozilla/5.0 (X11; Linux x86_64)"))
        assertTrue(desktop.contains("Chrome/"))
    }
}

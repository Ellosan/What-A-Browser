package com.whatabrowser.wat.webview

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The gate on Tor mode. Everything that is not a clear yes has to read as no.
 */
class TorCheckTest {

    @Test
    fun `the real answer is understood, quoted or not`() {
        assertTrue(TorCheck.isTor("""{"IsTor":true,"IP":"185.220.101.1"}"""))
        // As `evaluateJavascript` hands it back: a JSON string, escaped.
        assertTrue(TorCheck.isTor(""""{\"IsTor\":true,\"IP\":\"185.220.101.1\"}""""))
        assertTrue(TorCheck.isTor("""{"IsTor" : true}"""))
    }

    @Test
    fun `a no is a no`() {
        assertFalse(TorCheck.isTor("""{"IsTor":false,"IP":"81.2.3.4"}"""))
        assertFalse(TorCheck.isTor(""""{\"IsTor\":false}""""))
    }

    @Test
    fun `anything unexpected is not Tor`() {
        assertFalse(TorCheck.isTor(null))
        assertFalse(TorCheck.isTor(""))
        assertFalse(TorCheck.isTor("null"))
        assertFalse(TorCheck.isTor("<html><body>502 Bad Gateway</body></html>"))
        // A captive portal answering every request with its own page.
        assertFalse(TorCheck.isTor("<html>Sign in to WiFi</html>"))
        assertFalse(TorCheck.isTor("""{"IsTor":"true"}"""))
        assertFalse(TorCheck.isTor("IsTor"))
    }
}

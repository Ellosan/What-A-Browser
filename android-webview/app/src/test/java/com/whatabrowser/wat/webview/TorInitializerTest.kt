package com.whatabrowser.wat.webview

import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The one thing holding the Tor window together that a compiler cannot check.
 *
 * kmp-tor registers its native libraries from an `androidx.startup` initializer,
 * which runs from a `ContentProvider` — and a provider is created only in the
 * process that hosts it. The Tor window is a process of its own, so the app has
 * to run that initializer itself, and the class is `internal` to the library, so
 * it can only be reached by name.
 *
 * A name in a string is a name that can rot on the next dependency bump. This
 * test fails the build instead, which is the whole point: the failure it replaces
 * was a Tor window on someone's phone reporting that libtor.so did not exist
 * while the file sat in the app's own library directory.
 */
class TorInitializerTest {

    @Test
    fun `the tor resource initializer is where the app expects it`() {
        val initializer = Class.forName(TorEngine.RESOURCE_INITIALIZER)
        assertTrue(
            "${TorEngine.RESOURCE_INITIALIZER} must be an androidx.startup Initializer",
            Class.forName("androidx.startup.Initializer").isAssignableFrom(initializer),
        )
    }
}

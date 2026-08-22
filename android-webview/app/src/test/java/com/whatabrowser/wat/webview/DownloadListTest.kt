package com.whatabrowser.wat.webview

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class DownloadListTest {

    private fun entry(
        id: Long = 1L,
        status: Int = DownloadList.SUCCESSFUL,
        downloaded: Long = 0L,
        total: Long = 0L,
        startedAt: Long = 0L,
        localUri: String? = "content://downloads/my_downloads/1",
    ) = DownloadList.Entry(
        id = id,
        title = "file-$id.bin",
        host = "example.com",
        status = status,
        reason = 0,
        downloaded = downloaded,
        total = total,
        mimeType = "application/octet-stream",
        localUri = localUri,
        sourceUrl = "https://example.com/file-$id.bin",
        startedAt = startedAt,
    )

    @Test
    fun `a size the server never sent is not zero per cent`() {
        assertEquals(-1, DownloadList.percent(4096L, DownloadList.UNKNOWN_SIZE))
        assertEquals(-1, DownloadList.percent(0L, 0L))
        assertEquals(-1, DownloadList.percent(-1L, 100L))
    }

    @Test
    fun `progress is a percentage of what there is`() {
        assertEquals(0, DownloadList.percent(0L, 100L))
        assertEquals(50, DownloadList.percent(50L, 100L))
        assertEquals(100, DownloadList.percent(100L, 100L))
        // A server that undercounts, which happens with compressed transfers.
        assertEquals(100, DownloadList.percent(120L, 100L))
    }

    @Test
    fun `a very large download does not overflow on the way to a percentage`() {
        val eightGigabytes = 8L * 1024 * 1024 * 1024
        assertEquals(50, DownloadList.percent(eightGigabytes / 2, eightGigabytes))
    }

    @Test
    fun `sizes read the way a file manager writes them`() {
        assertEquals("0 B", DownloadList.size(0L))
        assertEquals("512 B", DownloadList.size(512L))
        assertEquals("1.0 KB", DownloadList.size(1024L))
        assertEquals("1.5 KB", DownloadList.size(1536L))
        assertEquals("1.0 MB", DownloadList.size(1024L * 1024))
        assertEquals("2.5 GB", DownloadList.size((2.5 * 1024 * 1024 * 1024).toLong()))
        assertEquals("?", DownloadList.size(DownloadList.UNKNOWN_SIZE))
    }

    @Test
    fun `a size is written the same way in every locale`() {
        val original = java.util.Locale.getDefault()
        try {
            // German writes decimals with a comma, which in a size reads as a
            // thousands separator: "1,5 KB" is a different number to most eyes.
            java.util.Locale.setDefault(java.util.Locale.GERMANY)
            assertEquals("1.5 KB", DownloadList.size(1536L))
        } finally {
            java.util.Locale.setDefault(original)
        }
    }

    @Test
    fun `the second line says how much of how much, when there is a how much`() {
        assertEquals("1.0 KB / 2.0 KB", DownloadList.transferred(1024L, 2048L))
        assertEquals("1.0 KB", DownloadList.transferred(1024L, DownloadList.UNKNOWN_SIZE))
    }

    @Test
    fun `what is still moving comes first, then the newest`() {
        val old = entry(id = 1L, startedAt = 100L)
        val recent = entry(id = 2L, startedAt = 300L)
        val running = entry(id = 3L, status = DownloadList.RUNNING, startedAt = 50L)
        val paused = entry(id = 4L, status = DownloadList.PAUSED, startedAt = 40L)

        val order = DownloadList.order(listOf(old, recent, running, paused)).map { it.id }

        assertEquals(listOf(3L, 4L, 2L, 1L), order)
    }

    @Test
    fun `a finished download with no file cannot be opened`() {
        assertTrue(entry().canOpen)
        assertFalse(entry(localUri = null).canOpen)
        assertFalse(entry(localUri = "").canOpen)
        assertFalse(entry(status = DownloadList.RUNNING).canOpen)
    }

    @Test
    fun `running, paused and finished are told apart`() {
        assertTrue(entry(status = DownloadList.RUNNING).isRunning)
        assertTrue(entry(status = DownloadList.PENDING).isRunning)
        assertFalse(entry(status = DownloadList.PAUSED).isRunning)
        assertTrue(entry(status = DownloadList.SUCCESSFUL).isFinished)
        assertTrue(entry(status = DownloadList.FAILED).isFinished)
        assertFalse(entry(status = DownloadList.PAUSED).isFinished)
    }

    @Test
    fun `the reasons worth naming are named`() {
        assertEquals("there is not enough room on the device", DownloadList.failure(1006))
        assertEquals("the server answered 404", DownloadList.failure(404))
        assertTrue(DownloadList.failure(9999).contains("9999"))
        assertEquals("waiting for a network", DownloadList.paused(2))
    }

    /**
     * The model copies `DownloadManager`'s constants so that it stays a plain
     * JVM file. This is the check that the copies are still the originals — the
     * numbers are stable API, but a wrong one here would silently show every
     * download as failed rather than fail to build.
     */
    @Test
    fun `the copied status numbers are the framework's`() {
        assertEquals(android.app.DownloadManager.STATUS_PENDING, DownloadList.PENDING)
        assertEquals(android.app.DownloadManager.STATUS_RUNNING, DownloadList.RUNNING)
        assertEquals(android.app.DownloadManager.STATUS_PAUSED, DownloadList.PAUSED)
        assertEquals(android.app.DownloadManager.STATUS_SUCCESSFUL, DownloadList.SUCCESSFUL)
        assertEquals(android.app.DownloadManager.STATUS_FAILED, DownloadList.FAILED)
    }
}

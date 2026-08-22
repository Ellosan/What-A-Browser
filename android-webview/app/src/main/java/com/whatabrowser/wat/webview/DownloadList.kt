package com.whatabrowser.wat.webview

/**
 * The downloads list, as a model: what is in it, how it is ordered, and how the
 * numbers read.
 *
 * Free of Android so that the arithmetic can be tested, which matters more here
 * than it looks. A progress bar that divides by a total of -1 — the download
 * manager's way of saying "the server never said how big this is" — is the usual
 * way a downloads screen shows 0% forever or crashes on a chunked response.
 *
 * The status and error numbers are copies of `android.app.DownloadManager`'s
 * constants. Copied rather than imported for the same reason: this file is a
 * plain JVM one. [DownloadsPanel] reads the real ones out of the cursor, and
 * `DownloadListTest` fails the build if the two ever disagree.
 */
object DownloadList {

    const val PENDING = 1
    const val RUNNING = 2
    const val PAUSED = 4
    const val SUCCESSFUL = 8
    const val FAILED = 16

    /** The download manager's "it did not say" for a content length. */
    const val UNKNOWN_SIZE = -1L

    class Entry(
        val id: Long,
        val title: String,
        val host: String,
        val status: Int,
        val reason: Int,
        val downloaded: Long,
        val total: Long,
        val mimeType: String?,
        val localUri: String?,
        val sourceUrl: String?,
        val startedAt: Long,
    ) {
        val isFinished: Boolean get() = status == SUCCESSFUL || status == FAILED
        val isRunning: Boolean get() = status == RUNNING || status == PENDING
        val canOpen: Boolean get() = status == SUCCESSFUL && !localUri.isNullOrEmpty()
    }

    /**
     * Newest first, with anything still moving at the top.
     *
     * A download that is running is the one being looked for; a list that sorts
     * strictly by time buries it under everything finished today.
     */
    fun order(entries: List<Entry>): List<Entry> =
        entries.sortedWith(
            compareByDescending<Entry> { it.isRunning || it.status == PAUSED }
                .thenByDescending { it.startedAt }
                .thenByDescending { it.id },
        )

    /**
     * How far along, out of a hundred — or -1 when the size is not known, which
     * is a bar that has to be indeterminate rather than a bar that reads zero.
     */
    fun percent(downloaded: Long, total: Long): Int {
        if (total <= 0L || downloaded < 0L) return -1
        if (downloaded >= total) return 100
        return ((downloaded * 100L) / total).toInt()
    }

    /**
     * Bytes, in the units a person reads.
     *
     * Powers of 1024 with the decimal names, which is what every browser's
     * downloads list shows and what the file manager on the same phone will
     * agree with.
     */
    fun size(bytes: Long): String {
        if (bytes < 0L) return "?"
        if (bytes < 1024L) return "$bytes B"
        val units = arrayOf("KB", "MB", "GB", "TB")
        var value = bytes.toDouble() / 1024.0
        var unit = 0
        while (value >= 1024.0 && unit < units.size - 1) {
            value /= 1024.0
            unit++
        }
        val rounded = if (value >= 100.0) {
            value.toLong().toString()
        } else {
            // One decimal, without String.format so the answer does not change
            // with the phone's locale — a comma here would be read as a
            // thousands separator by half the world.
            val tenths = Math.round(value * 10.0)
            "${tenths / 10}.${tenths % 10}"
        }
        return "$rounded ${units[unit]}"
    }

    /** `3.4 MB of 12.0 MB`, or just what has arrived when nobody said how much. */
    fun transferred(downloaded: Long, total: Long): String =
        if (total > 0L) "${size(downloaded)} / ${size(total)}" else size(downloaded)

    /**
     * Why it failed, in words.
     *
     * The numbers are the download manager's, and most of them mean nothing to
     * anyone; the ones that do — no room, no network, a server that refused —
     * are worth saying plainly, because they each have a different answer.
     */
    fun failure(reason: Int): String = when (reason) {
        1001 -> "the file could not be written"
        1002 -> "the server answered in a way the download manager could not read"
        1004 -> "the transfer broke"
        1006 -> "there is not enough room on the device"
        1007 -> "no storage was available"
        1008 -> "the server does not support resuming"
        1009 -> "a file of that name is already there"
        1005 -> "too many redirects"
        // An HTTP status the manager could not handle is passed through as the
        // status itself, which is more useful than any word for it.
        in 400..599 -> "the server answered $reason"
        else -> "an unknown error ($reason)"
    }

    /** Why it is waiting. */
    fun paused(reason: Int): String = when (reason) {
        1 -> "waiting to retry"
        2 -> "waiting for a network"
        3 -> "waiting for Wi-Fi"
        4 -> "paused"
        else -> "paused"
    }
}

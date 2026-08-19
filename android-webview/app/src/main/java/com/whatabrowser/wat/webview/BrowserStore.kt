package com.whatabrowser.wat.webview

import android.content.ContentValues
import android.content.Context
import android.database.sqlite.SQLiteDatabase
import android.database.sqlite.SQLiteOpenHelper
import android.os.Handler
import android.os.Looper
import java.util.concurrent.Executors

/**
 * History and bookmarks, in a small SQLite file.
 *
 * Two rules shape this. Writes never touch the main thread — a visit is recorded
 * on every page load, and a disk write in the middle of a navigation is exactly
 * the kind of stall this build exists to avoid. And nothing opens the database
 * during startup: the connection is created the first time something is written
 * or read, which for a cold start that goes straight to a page is after the first
 * frame.
 *
 * History is one row per address, holding the most recent visit, which is what
 * makes the list read like a list of pages rather than a log of loads.
 */
class BrowserStore(context: Context) {

    class Entry(val url: String, val title: String, val bookmarked: Boolean = false) {
        val label: String get() = title.ifBlank { url }
    }

    private val helper = Helper(context.applicationContext)

    private val worker = Executors.newSingleThreadExecutor { runnable ->
        Thread(runnable, "browser-store").apply { isDaemon = true }
    }

    private val main = Handler(Looper.getMainLooper())

    /** Suppresses the reload-and-record loop a page refresh would otherwise make. */
    private var lastRecorded: String? = null

    private var visitsSinceTrim = 0

    /**
     * Records a visit, off the main thread.
     *
     * Only pages the browser actually rendered: an address that was refused, or
     * `about:blank` between tabs, is not somewhere the reader has been.
     */
    fun recordVisit(url: String, title: String) {
        if (UrlResolver.decide(url) != UrlResolver.Decision.RENDER) return
        if (url == lastRecorded) return
        lastRecorded = url
        val now = System.currentTimeMillis()
        submit {
            it.insertWithOnConflict(
                TABLE_HISTORY,
                null,
                ContentValues().apply {
                    put("url", url)
                    put("title", title)
                    put("visited_at", now)
                },
                SQLiteDatabase.CONFLICT_REPLACE,
            )
            if (++visitsSinceTrim >= TRIM_EVERY) {
                visitsSinceTrim = 0
                // Unbounded history is a growing file and a slowing query for
                // something nobody scrolls back through.
                it.execSQL(
                    "DELETE FROM $TABLE_HISTORY WHERE url NOT IN " +
                        "(SELECT url FROM $TABLE_HISTORY ORDER BY visited_at DESC LIMIT ?)",
                    arrayOf(HISTORY_LIMIT.toString()),
                )
            }
        }
    }

    /** The title arrives after the page has already been recorded. */
    fun recordTitle(url: String, title: String) {
        if (title.isBlank()) return
        submit {
            it.update(
                TABLE_HISTORY,
                ContentValues().apply { put("title", title) },
                "url = ?",
                arrayOf(url),
            )
        }
    }

    fun history(limit: Int = 300): List<Entry> = read(
        "SELECT url, title FROM $TABLE_HISTORY ORDER BY visited_at DESC LIMIT ?",
        arrayOf(limit.toString()),
    )

    fun clearHistory() {
        lastRecorded = null
        submit { it.delete(TABLE_HISTORY, null, null) }
    }

    fun bookmarks(): List<Entry> = read(
        "SELECT url, title FROM $TABLE_BOOKMARKS ORDER BY created_at DESC",
        emptyArray(),
    ).map { Entry(it.url, it.title, bookmarked = true) }

    /**
     * Whether a page is bookmarked, answered off the main thread.
     *
     * This is asked on every navigation, to draw the star. Asking synchronously
     * would open the database during the first page load of a cold start, which
     * is the one moment this browser is built to keep clear.
     */
    fun bookmarked(url: String, then: (Boolean) -> Unit) {
        worker.execute {
            val answer = try {
                isBookmarked(url)
            } catch (_: Exception) {
                false
            }
            main.post { then(answer) }
        }
    }

    fun isBookmarked(url: String): Boolean = helper.readableDatabase
        .rawQuery("SELECT 1 FROM $TABLE_BOOKMARKS WHERE url = ? LIMIT 1", arrayOf(url))
        .use { it.moveToFirst() }

    /** Adds or removes the bookmark, and reports which it did. */
    fun toggleBookmark(url: String, title: String): Boolean {
        if (UrlResolver.decide(url) != UrlResolver.Decision.RENDER) return false
        val nowBookmarked = !isBookmarked(url)
        val now = System.currentTimeMillis()
        submit {
            if (nowBookmarked) {
                it.insertWithOnConflict(
                    TABLE_BOOKMARKS,
                    null,
                    ContentValues().apply {
                        put("url", url)
                        put("title", title)
                        put("created_at", now)
                    },
                    SQLiteDatabase.CONFLICT_REPLACE,
                )
            } else {
                it.delete(TABLE_BOOKMARKS, "url = ?", arrayOf(url))
            }
        }
        return nowBookmarked
    }

    fun removeBookmark(url: String) {
        submit { it.delete(TABLE_BOOKMARKS, "url = ?", arrayOf(url)) }
    }

    /**
     * What to offer for something half-typed: bookmarks first, then history.
     *
     * Called from `AutoCompleteTextView`'s filter, which runs on a worker thread
     * of its own — which is why this one is allowed to be synchronous.
     */
    fun suggest(query: String, limit: Int = 8): List<Entry> {
        val like = "%" + query.replace("%", "").replace("_", "") + "%"
        val marks = read(
            "SELECT url, title FROM $TABLE_BOOKMARKS WHERE url LIKE ? OR title LIKE ? " +
                "ORDER BY created_at DESC LIMIT ?",
            arrayOf(like, like, limit.toString()),
        ).map { Entry(it.url, it.title, bookmarked = true) }

        val seen = marks.map { it.url }.toMutableSet()
        val visited = read(
            "SELECT url, title FROM $TABLE_HISTORY WHERE url LIKE ? OR title LIKE ? " +
                "ORDER BY visited_at DESC LIMIT ?",
            arrayOf(like, like, (limit * 2).toString()),
        ).filter { seen.add(it.url) }

        return (marks + visited).take(limit)
    }

    private fun read(sql: String, args: Array<String>): List<Entry> = try {
        helper.readableDatabase.rawQuery(sql, args).use { cursor ->
            val out = ArrayList<Entry>(cursor.count)
            while (cursor.moveToNext()) {
                out.add(Entry(cursor.getString(0), cursor.getString(1)))
            }
            out
        }
    } catch (_: Exception) {
        // A corrupt or unreadable database must not take the browser with it;
        // an empty list means "no history", which is recoverable by using it.
        emptyList()
    }

    private fun submit(body: (SQLiteDatabase) -> Unit) {
        worker.execute {
            try {
                body(helper.writableDatabase)
            } catch (_: Exception) {
                // Same reasoning: losing a history row is not worth a crash.
            }
        }
    }

    private class Helper(context: Context) :
        SQLiteOpenHelper(context, "browser.db", null, VERSION) {

        override fun onCreate(db: SQLiteDatabase) {
            db.execSQL(
                "CREATE TABLE $TABLE_HISTORY (" +
                    "url TEXT PRIMARY KEY, title TEXT NOT NULL, visited_at INTEGER NOT NULL)",
            )
            db.execSQL("CREATE INDEX history_visited ON $TABLE_HISTORY (visited_at)")
            db.execSQL(
                "CREATE TABLE $TABLE_BOOKMARKS (" +
                    "url TEXT PRIMARY KEY, title TEXT NOT NULL, created_at INTEGER NOT NULL)",
            )
        }

        override fun onUpgrade(db: SQLiteDatabase, oldVersion: Int, newVersion: Int) {
            // Nothing to migrate from yet.
        }
    }

    private companion object {
        const val VERSION = 1
        const val TABLE_HISTORY = "history"
        const val TABLE_BOOKMARKS = "bookmarks"
        const val HISTORY_LIMIT = 3000
        const val TRIM_EVERY = 50
    }
}

package com.whatabrowser.wat.webview

import android.app.Activity
import android.app.AlertDialog
import android.app.Dialog
import android.app.DownloadManager
import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.content.Intent
import android.database.Cursor
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.Window
import android.widget.BaseAdapter
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ListView
import android.widget.ProgressBar
import android.widget.TextView
import android.widget.Toast

/**
 * The downloads page.
 *
 * Everything this browser has downloaded, in the browser, rather than a button
 * that throws the reader out into the system's downloads app and hopes. Each row
 * shows what it is, where it came from and how far along; tapping a finished one
 * opens it, holding any of them offers the rest — share, copy the address, try
 * again, delete.
 *
 * The list is the download manager's, read through a cursor each time rather
 * than mirrored into a database of our own. A second copy of this state is a
 * second thing to be wrong: the manager keeps running when the browser is
 * closed, and a row that says "downloading" three days later is exactly what a
 * mirror produces.
 *
 * While anything is moving the list re-reads itself once a second. It stops the
 * moment the dialog goes away, and it does not run at all when everything is
 * finished — a downloads screen that polls forever is a downloads screen that
 * costs battery for a list that cannot change.
 */
object DownloadsPanel {

    private const val REFRESH_MS = 1_000L

    fun show(activity: Activity): Dialog {
        val palette = GlassSurface.palette(activity)
        val pad = GlassSurface.dp(activity, 18f).toInt()

        val dialog = Dialog(activity)
        dialog.requestWindowFeature(Window.FEATURE_NO_TITLE)

        val root = LinearLayout(activity).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(pad, pad, pad, pad / 2)
        }
        root.addView(
            TextView(activity).apply {
                text = activity.getString(R.string.downloads)
                setTextColor(palette.text)
                textSize = 18f
            },
        )

        val empty = TextView(activity).apply {
            text = activity.getString(R.string.downloads_empty)
            setTextColor(palette.textMuted)
            textSize = 13f
            setPadding(0, pad / 2, 0, pad / 2)
            visibility = View.GONE
        }

        val adapter = Adapter(activity, palette)
        val list = ListView(activity).apply {
            this.adapter = adapter
            divider = null
            dividerHeight = 0
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                (activity.resources.displayMetrics.heightPixels * 0.55f).toInt(),
            )
        }
        root.addView(empty)
        root.addView(list)

        adapter.onReloaded = { rows ->
            empty.visibility = if (rows == 0) View.VISIBLE else View.GONE
            list.visibility = if (rows == 0) View.GONE else View.VISIBLE
        }

        list.setOnItemClickListener { _, _, position, _ ->
            val entry = adapter.entryAt(position) ?: return@setOnItemClickListener
            if (entry.canOpen) open(activity, entry) else actions(activity, entry, adapter::reload)
        }
        list.setOnItemLongClickListener { _, _, position, _ ->
            val entry = adapter.entryAt(position) ?: return@setOnItemLongClickListener false
            actions(activity, entry, adapter::reload)
            true
        }

        val buttons = LinearLayout(activity).apply {
            orientation = LinearLayout.HORIZONTAL
        }
        buttons.addView(
            Button(activity).apply {
                text = activity.getString(R.string.downloads_system)
                setTextColor(palette.accent)
                background = null
                gravity = Gravity.START or Gravity.CENTER_VERTICAL
                layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
                setOnClickListener { DownloadQueue.showAll(activity) }
            },
        )
        buttons.addView(
            Button(activity).apply {
                text = activity.getString(R.string.downloads_clear)
                setTextColor(palette.accent)
                background = null
                gravity = Gravity.END or Gravity.CENTER_VERTICAL
                layoutParams = LinearLayout.LayoutParams(0, ViewGroup.LayoutParams.WRAP_CONTENT, 1f)
                setOnClickListener { clearFinished(activity, adapter) }
            },
        )
        root.addView(buttons)

        dialog.setContentView(root)
        dialog.window?.apply {
            setBackgroundDrawable(GlassSurface.panel(activity, GlassGeometry.radiusLarge))
            setLayout(
                (activity.resources.displayMetrics.widthPixels * 0.94f).toInt(),
                ViewGroup.LayoutParams.WRAP_CONTENT,
            )
        }

        // The ticking is tied to the dialog, not to the activity: a dismissed
        // dialog leaves nothing behind to keep waking up.
        val handler = Handler(Looper.getMainLooper())
        val tick = object : Runnable {
            override fun run() {
                if (!dialog.isShowing) return
                if (adapter.reload()) handler.postDelayed(this, REFRESH_MS)
            }
        }
        dialog.setOnShowListener {
            if (adapter.reload()) handler.postDelayed(tick, REFRESH_MS)
        }
        dialog.setOnDismissListener { handler.removeCallbacks(tick) }

        adapter.reload()
        return dialog
    }

    /** Everything this app has downloaded, as the model sees it. */
    fun entries(context: Context): List<DownloadList.Entry> {
        val manager = context.getSystemService(Context.DOWNLOAD_SERVICE) as? DownloadManager
            ?: return emptyList()
        val cursor: Cursor = runCatching { manager.query(DownloadManager.Query()) }.getOrNull()
            ?: return emptyList()
        val found = mutableListOf<DownloadList.Entry>()
        cursor.use {
            val id = it.getColumnIndex(DownloadManager.COLUMN_ID)
            val title = it.getColumnIndex(DownloadManager.COLUMN_TITLE)
            val description = it.getColumnIndex(DownloadManager.COLUMN_DESCRIPTION)
            val status = it.getColumnIndex(DownloadManager.COLUMN_STATUS)
            val reason = it.getColumnIndex(DownloadManager.COLUMN_REASON)
            val soFar = it.getColumnIndex(DownloadManager.COLUMN_BYTES_DOWNLOADED_SO_FAR)
            val total = it.getColumnIndex(DownloadManager.COLUMN_TOTAL_SIZE_BYTES)
            val mime = it.getColumnIndex(DownloadManager.COLUMN_MEDIA_TYPE)
            val local = it.getColumnIndex(DownloadManager.COLUMN_LOCAL_URI)
            val source = it.getColumnIndex(DownloadManager.COLUMN_URI)
            val stamp = it.getColumnIndex(DownloadManager.COLUMN_LAST_MODIFIED_TIMESTAMP)
            while (it.moveToNext()) {
                found += DownloadList.Entry(
                    id = it.getLong(id),
                    title = it.getStringOrEmpty(title),
                    host = it.getStringOrEmpty(description),
                    status = it.getInt(status),
                    reason = it.getInt(reason),
                    downloaded = it.getLong(soFar),
                    total = it.getLong(total),
                    mimeType = it.getStringOrNull(mime),
                    localUri = it.getStringOrNull(local),
                    sourceUrl = it.getStringOrNull(source),
                    startedAt = it.getLong(stamp),
                )
            }
        }
        return DownloadList.order(found)
    }

    private fun Cursor.getStringOrEmpty(column: Int): String =
        if (column < 0 || isNull(column)) "" else getString(column).orEmpty()

    private fun Cursor.getStringOrNull(column: Int): String? =
        if (column < 0 || isNull(column)) null else getString(column)

    /**
     * Opens a finished download.
     *
     * Through the download manager's own content URI, with a read grant attached
     * — never a `file://` path. Handing another app a file path to a file it
     * cannot read is how "no app can open this" happens for a file that opens
     * fine from the notification.
     */
    private fun open(activity: Activity, entry: DownloadList.Entry) {
        val uri = contentUri(activity, entry) ?: run {
            Toast.makeText(activity, R.string.download_gone, Toast.LENGTH_SHORT).show()
            return
        }
        val intent = Intent(Intent.ACTION_VIEW)
            .setDataAndType(uri, entry.mimeType ?: "*/*")
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        try {
            activity.startActivity(intent)
        } catch (_: ActivityNotFoundException) {
            Toast.makeText(activity, R.string.download_no_opener, Toast.LENGTH_LONG).show()
        }
    }

    private fun contentUri(activity: Activity, entry: DownloadList.Entry): Uri? {
        val manager = activity.getSystemService(Context.DOWNLOAD_SERVICE) as? DownloadManager
        val fromManager = runCatching { manager?.getUriForDownloadedFile(entry.id) }.getOrNull()
        if (fromManager != null && fromManager.scheme == "content") return fromManager
        // Before the manager started answering with content URIs this was a path.
        // Rather than hand a path to another app, the row's own content URI is
        // built from its id, which is the same thing the manager returns today.
        return runCatching {
            Uri.withAppendedPath(Uri.parse("content://downloads/my_downloads"), entry.id.toString())
        }.getOrNull()
    }

    /** Share, copy the address, try again, delete. */
    private fun actions(activity: Activity, entry: DownloadList.Entry, onChanged: () -> Unit) {
        val labels = mutableListOf<String>()
        val runs = mutableListOf<() -> Unit>()

        if (entry.canOpen) {
            labels += activity.getString(R.string.open)
            runs += { open(activity, entry) }
            labels += activity.getString(R.string.share_page)
            runs += { share(activity, entry) }
        }
        if (!entry.sourceUrl.isNullOrEmpty()) {
            labels += activity.getString(R.string.copy_address)
            runs += { copy(activity, entry) }
        }
        if (entry.status == DownloadList.FAILED && !entry.sourceUrl.isNullOrEmpty()) {
            labels += activity.getString(R.string.download_retry)
            runs += {
                remove(activity, entry)
                DownloadQueue.start(activity, entry.sourceUrl, null, null, entry.mimeType)
                onChanged()
            }
        }
        if (entry.isRunning || entry.status == DownloadList.PAUSED) {
            labels += activity.getString(R.string.download_cancel)
            runs += {
                remove(activity, entry)
                onChanged()
            }
        } else {
            labels += activity.getString(R.string.download_delete)
            runs += {
                remove(activity, entry)
                onChanged()
            }
        }

        AlertDialog.Builder(activity)
            .setTitle(entry.title.ifEmpty { activity.getString(R.string.downloads) })
            .setItems(labels.toTypedArray()) { _, which -> runs.getOrNull(which)?.invoke() }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private fun share(activity: Activity, entry: DownloadList.Entry) {
        val uri = contentUri(activity, entry) ?: return
        val intent = Intent(Intent.ACTION_SEND)
            .setType(entry.mimeType ?: "*/*")
            .putExtra(Intent.EXTRA_STREAM, uri)
            .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
        runCatching {
            activity.startActivity(
                Intent.createChooser(intent, activity.getString(R.string.share_page)),
            )
        }
    }

    private fun copy(activity: Activity, entry: DownloadList.Entry) {
        val clipboard = activity.getSystemService(Context.CLIPBOARD_SERVICE) as? ClipboardManager ?: return
        clipboard.setPrimaryClip(ClipData.newPlainText(entry.title, entry.sourceUrl))
        Toast.makeText(activity, R.string.copied, Toast.LENGTH_SHORT).show()
    }

    /** Removes the row, and the file with it — which is what `remove` does. */
    private fun remove(activity: Activity, entry: DownloadList.Entry) {
        val manager = activity.getSystemService(Context.DOWNLOAD_SERVICE) as? DownloadManager ?: return
        runCatching { manager.remove(entry.id) }
    }

    private fun clearFinished(activity: Activity, adapter: Adapter) {
        val finished = entries(activity).filter { it.isFinished }
        if (finished.isEmpty()) {
            Toast.makeText(activity, R.string.downloads_nothing_to_clear, Toast.LENGTH_SHORT).show()
            return
        }
        AlertDialog.Builder(activity)
            .setTitle(R.string.downloads_clear)
            .setMessage(activity.getString(R.string.downloads_clear_ask, finished.size))
            .setPositiveButton(R.string.downloads_delete_files) { _, _ ->
                for (entry in finished) remove(activity, entry)
                adapter.reload()
            }
            .setNegativeButton(android.R.string.cancel, null)
            .show()
    }

    private class Adapter(
        private val activity: Activity,
        private val palette: GlassPalette,
    ) : BaseAdapter() {

        private var rows: List<DownloadList.Entry> = emptyList()

        var onReloaded: (Int) -> Unit = {}

        fun entryAt(position: Int): DownloadList.Entry? = rows.getOrNull(position)

        /** Re-reads the list. Answers whether anything in it is still moving. */
        fun reload(): Boolean {
            rows = entries(activity)
            notifyDataSetChanged()
            onReloaded(rows.size)
            return rows.any { it.isRunning }
        }

        override fun getCount() = rows.size

        override fun getItem(position: Int) = rows[position]

        override fun getItemId(position: Int) = rows[position].id

        override fun getView(position: Int, convertView: View?, parent: ViewGroup): View {
            val entry = rows[position]
            val pad = GlassSurface.dp(activity, 8f).toInt()

            val name = TextView(activity).apply {
                text = entry.title.ifEmpty { activity.getString(R.string.downloads) }
                setTextColor(palette.text)
                textSize = 15f
                maxLines = 1
                ellipsize = android.text.TextUtils.TruncateAt.MIDDLE
            }

            val detail = TextView(activity).apply {
                text = describe(entry)
                setTextColor(
                    if (entry.status == DownloadList.FAILED) palette.warning else palette.textMuted,
                )
                textSize = 11f
            }

            val column = LinearLayout(activity).apply {
                orientation = LinearLayout.VERTICAL
                setPadding(pad, pad / 2, pad, pad / 2)
                addView(name)
                addView(detail)
            }

            if (entry.isRunning || entry.status == DownloadList.PAUSED) {
                val done = DownloadList.percent(entry.downloaded, entry.total)
                column.addView(
                    ProgressBar(activity, null, android.R.attr.progressBarStyleHorizontal).apply {
                        isIndeterminate = done < 0
                        if (done >= 0) {
                            max = 100
                            progress = done
                        }
                        layoutParams = LinearLayout.LayoutParams(
                            ViewGroup.LayoutParams.MATCH_PARENT,
                            GlassSurface.dp(activity, 3f).toInt(),
                        ).apply { topMargin = pad / 2 }
                    },
                )
            }

            return column
        }

        /** The second line: where it came from, and where it has got to. */
        private fun describe(entry: DownloadList.Entry): String {
            val where = entry.host.ifEmpty { UrlResolver.hostOf(entry.sourceUrl.orEmpty()) }
            val state = when (entry.status) {
                DownloadList.SUCCESSFUL -> DownloadList.size(entry.total.coerceAtLeast(entry.downloaded))
                DownloadList.FAILED ->
                    activity.getString(R.string.download_failed_because, DownloadList.failure(entry.reason))
                DownloadList.PAUSED -> DownloadList.paused(entry.reason)
                DownloadList.PENDING -> activity.getString(R.string.download_starting)
                else -> DownloadList.transferred(entry.downloaded, entry.total)
            }
            return if (where.isEmpty()) state else "$where  ·  $state"
        }
    }
}

package com.whatabrowser.wat.webview

import android.app.Activity
import android.app.DownloadManager
import android.content.Intent
import android.net.Uri
import android.os.Environment
import android.webkit.CookieManager
import android.widget.Toast

/**
 * Saving a file, through the system's download manager.
 *
 * Two decisions worth stating. The file goes to the app's own external files
 * directory rather than the shared Downloads folder, because writing to the
 * shared one needs `WRITE_EXTERNAL_STORAGE` on Android 9 and below — a permission
 * this browser does not ask for and will not start asking for to save a PDF. The
 * download still appears in the system's Downloads list, and can be opened and
 * shared from there.
 *
 * And the name comes from [Downloads], never from the header directly. See that
 * file for what a server can otherwise talk a browser into writing.
 */
object DownloadQueue {

    fun start(
        activity: Activity,
        url: String,
        userAgent: String?,
        contentDisposition: String?,
        mimeType: String?,
    ) {
        // `blob:` and `data:` downloads are generated inside the page and cannot
        // be re-fetched from outside it; the download manager only speaks http.
        if (UrlResolver.decide(url) != UrlResolver.Decision.RENDER) {
            Toast.makeText(activity, R.string.download_unsupported, Toast.LENGTH_SHORT).show()
            return
        }

        val name = Downloads.fileName(url, contentDisposition, mimeType)
        val manager = activity.getSystemService(Activity.DOWNLOAD_SERVICE) as? DownloadManager
        if (manager == null) {
            Toast.makeText(activity, R.string.download_failed, Toast.LENGTH_SHORT).show()
            return
        }

        try {
            val request = DownloadManager.Request(Uri.parse(url))
                .setTitle(name)
                .setDescription(UrlResolver.hostOf(url))
                .setNotificationVisibility(DownloadManager.Request.VISIBILITY_VISIBLE_NOTIFY_COMPLETED)
                .setDestinationInExternalFilesDir(activity, Environment.DIRECTORY_DOWNLOADS, name)

            // The download leaves the WebView and is fetched again by another
            // process, so it has to carry the session with it or an authenticated
            // file comes back as a login page.
            userAgent?.let { request.addRequestHeader("User-Agent", it) }
            CookieManager.getInstance().getCookie(url)?.let { request.addRequestHeader("Cookie", it) }
            mimeType?.let { request.setMimeType(it) }

            manager.enqueue(request)
            Toast.makeText(
                activity,
                activity.getString(R.string.download_started, name),
                Toast.LENGTH_SHORT,
            ).show()
        } catch (_: Exception) {
            // An unsupported URL, or a download manager that has been disabled on
            // the device — both are the user's problem to see, not a crash.
            Toast.makeText(activity, R.string.download_failed, Toast.LENGTH_SHORT).show()
        }
    }

    /** The system's own list of downloads, which is where these files show up. */
    fun showAll(activity: Activity) {
        try {
            activity.startActivity(
                Intent(DownloadManager.ACTION_VIEW_DOWNLOADS)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            )
        } catch (_: Exception) {
            Toast.makeText(activity, R.string.download_no_viewer, Toast.LENGTH_SHORT).show()
        }
    }
}

package com.whatabrowser.wat.webview

import android.app.Activity
import android.app.AlertDialog
import android.webkit.JsPromptResult
import android.webkit.JsResult
import android.widget.EditText
import android.widget.FrameLayout

/**
 * `alert`, `confirm`, `prompt` and "are you sure you want to leave", drawn by the
 * browser.
 *
 * These exist because a page whose dialogs never appear is a page that hangs:
 * script stops at `confirm()` until something answers it. Every one of them names
 * the site that is asking, because a dialog with no attribution is how a page
 * pretends to be the browser or the operating system.
 *
 * Each returns true — the browser handled it — and every path answers the
 * `JsResult`, including the one where the dialog is dismissed with back.
 */
object JsDialogs {

    fun alert(activity: Activity, url: String?, message: String?, result: JsResult): Boolean {
        if (activity.isFinishing) {
            result.cancel()
            return true
        }
        builder(activity, url)
            .setMessage(message.orEmpty())
            .setPositiveButton(android.R.string.ok) { _, _ -> result.confirm() }
            .setOnCancelListener { result.cancel() }
            .show()
        return true
    }

    fun confirm(activity: Activity, url: String?, message: String?, result: JsResult): Boolean {
        if (activity.isFinishing) {
            result.cancel()
            return true
        }
        builder(activity, url)
            .setMessage(message.orEmpty())
            .setPositiveButton(android.R.string.ok) { _, _ -> result.confirm() }
            .setNegativeButton(android.R.string.cancel) { _, _ -> result.cancel() }
            .setOnCancelListener { result.cancel() }
            .show()
        return true
    }

    fun prompt(
        activity: Activity,
        url: String?,
        message: String?,
        defaultValue: String?,
        result: JsPromptResult,
    ): Boolean {
        if (activity.isFinishing) {
            result.cancel()
            return true
        }
        val input = EditText(activity).apply {
            setText(defaultValue.orEmpty())
            setSingleLine()
        }
        val padding = GlassSurface.dp(activity, 20f).toInt()
        val holder = FrameLayout(activity).apply {
            setPadding(padding, padding / 2, padding, 0)
            addView(input)
        }
        builder(activity, url)
            .setMessage(message.orEmpty())
            .setView(holder)
            .setPositiveButton(android.R.string.ok) { _, _ -> result.confirm(input.text.toString()) }
            .setNegativeButton(android.R.string.cancel) { _, _ -> result.cancel() }
            .setOnCancelListener { result.cancel() }
            .show()
        return true
    }

    /**
     * The one dialog the page does not get to word itself.
     *
     * `onbeforeunload` messages are a favourite of pages that do not want to be
     * left, so only the browser's own question is shown.
     */
    fun beforeUnload(activity: Activity, url: String?, result: JsResult): Boolean {
        if (activity.isFinishing) {
            result.confirm()
            return true
        }
        builder(activity, url)
            .setMessage(activity.getString(R.string.leave_page_message))
            .setPositiveButton(R.string.leave_page) { _, _ -> result.confirm() }
            .setNegativeButton(R.string.stay_on_page) { _, _ -> result.cancel() }
            .setOnCancelListener { result.cancel() }
            .show()
        return true
    }

    private fun builder(activity: Activity, url: String?): AlertDialog.Builder =
        AlertDialog.Builder(activity).setTitle(
            activity.getString(R.string.site_says, UrlResolver.hostOf(url.orEmpty())),
        )
}

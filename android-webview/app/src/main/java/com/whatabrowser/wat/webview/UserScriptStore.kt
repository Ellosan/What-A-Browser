package com.whatabrowser.wat.webview

import android.content.Context
import java.io.File
import java.net.HttpURLConnection
import java.net.URL

/**
 * The userscripts that are installed, on disk.
 *
 * One file per script under `filesDir/userscripts`, which makes them findable,
 * removable and backup-free, and which means a script is a file rather than a
 * row in a database nobody can read. Whether each is switched on lives in the
 * preferences beside the rest of the settings.
 *
 * Scripts are stored, never fetched at load time. A browser that downloaded a
 * script every time it opened a page would be running whatever that address
 * serves today, which is a remote code execution channel with extra steps —
 * installing is a deliberate act, and after it the copy on disk is what runs.
 */
class UserScriptStore(context: Context) {

    private val directory = File(context.filesDir, "userscripts").apply { mkdirs() }

    private val prefs = context.applicationContext
        .getSharedPreferences("userscripts", Context.MODE_PRIVATE)

    class Installed(val file: File, val script: UserScript, val enabled: Boolean)

    fun all(): List<Installed> = directory
        .listFiles { file -> file.isFile && file.name.endsWith(SUFFIX) }
        .orEmpty()
        .sortedBy { it.name }
        .mapNotNull { file ->
            val source = runCatching { file.readText() }.getOrNull() ?: return@mapNotNull null
            val script = UserScript.parse(source) ?: return@mapNotNull null
            Installed(file, script, prefs.getBoolean(file.name, true))
        }

    /** The enabled scripts that apply to [url], in the order they were installed. */
    fun forUrl(url: String): List<UserScript> =
        all().filter { it.enabled && it.script.appliesTo(url) }.map { it.script }

    fun setEnabled(installed: Installed, enabled: Boolean) =
        prefs.edit().putBoolean(installed.file.name, enabled).apply()

    fun remove(installed: Installed) {
        installed.file.delete()
        prefs.edit().remove(installed.file.name).apply()
    }

    /**
     * Saves a script, replacing one of the same name.
     *
     * Returns what was parsed, or null if the text is not a userscript — which
     * is checked here rather than at load, so nothing unreadable can end up in
     * the directory.
     */
    fun install(source: String): UserScript? {
        val script = UserScript.parse(source) ?: return null
        val file = File(directory, fileNameFor(script.name))
        return runCatching {
            file.writeText(source)
            prefs.edit().putBoolean(file.name, true).apply()
            script
        }.getOrNull()
    }

    /**
     * Fetches a script over HTTPS and installs it.
     *
     * Runs on the caller's thread, which must not be the main one. HTTPS only,
     * no redirects to anything else, and a hard size cap: this is the one place
     * the browser takes code from the network on purpose, so the rules around it
     * are worth being narrow.
     */
    fun installFrom(address: String): Result<UserScript> {
        if (!address.startsWith("https://")) {
            return Result.failure(IllegalArgumentException("a userscript has to come over https"))
        }
        return runCatching {
            val connection = URL(address).openConnection() as HttpURLConnection
            connection.connectTimeout = 20_000
            connection.readTimeout = 20_000
            connection.instanceFollowRedirects = true
            connection.setRequestProperty("Accept", "text/plain, application/javascript, */*")
            try {
                if (connection.responseCode !in 200..299) {
                    error("the server answered ${connection.responseCode}")
                }
                // The redirect chain may have left HTTPS; the check above only
                // covered the address that was typed.
                if (connection.url.protocol != "https") {
                    error("that redirected somewhere that is not https")
                }
                val source = connection.inputStream.use { stream ->
                    val buffer = ByteArray(UserScript.MAX_SOURCE + 1)
                    var read = 0
                    while (read < buffer.size) {
                        val count = stream.read(buffer, read, buffer.size - read)
                        if (count < 0) break
                        read += count
                    }
                    if (read > UserScript.MAX_SOURCE) error("that file is too large to be a userscript")
                    String(buffer, 0, read, Charsets.UTF_8)
                }
                install(source) ?: error("that is not a userscript: no metadata block")
            } finally {
                connection.disconnect()
            }
        }
    }

    /** A file name that cannot leave the directory, from a name the script chose. */
    private fun fileNameFor(name: String): String {
        val safe = name.map { char ->
            if (char.isLetterOrDigit() || char == '-' || char == '_') char else '-'
        }.joinToString("").trim('-').take(60)
        return (safe.ifEmpty { "script" }) + SUFFIX
    }

    private companion object {
        const val SUFFIX = ".user.js"
    }
}

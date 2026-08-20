package com.whatabrowser.wat.webview

import java.io.ByteArrayOutputStream
import java.nio.charset.Charset

/**
 * Where a download's file name comes from, and why none of it is taken at face
 * value.
 *
 * The name of a downloaded file is chosen by the server, in a header, and a
 * server that says `Content-Disposition: attachment; filename="../../../shared_prefs/settings.xml"`
 * is asking the browser to write outside the download directory. Android's own
 * `URLUtil.guessFileName` has had this bug more than once, so the parsing is
 * here, in pure Kotlin, where a JVM test can prove what it does with a hostile
 * header.
 *
 * The rule is: whatever the header says, only the last path segment survives,
 * and it is stripped down to something that cannot mean anything but a file name.
 */
object Downloads {

    /** Long enough for real names, short enough to survive any file system. */
    private const val MAX_LENGTH = 100

    private const val FALLBACK = "download"

    /**
     * The file name to save a download as.
     *
     * @param url the URL being downloaded, used when the server offers no name
     * @param contentDisposition the raw header, exactly as the server sent it
     * @param mimeType the type the server declared, used only to add a missing
     *   extension
     */
    fun fileName(url: String, contentDisposition: String?, mimeType: String?): String {
        val offered = contentDisposition?.let(::nameFromDisposition)
        val name = sanitize(offered) ?: sanitize(nameFromUrl(url)) ?: FALLBACK
        return clamp(withExtension(name, mimeType))
    }

    /**
     * The name out of a `Content-Disposition` header, undecoded but unparsed.
     *
     * RFC 6266: `filename*` wins over `filename` when both are present, because
     * it is the one that can carry non-ASCII. Sending both — a plain ASCII
     * `filename` and a hostile `filename*` — is one way people have got past
     * parsers that only looked at the first.
     */
    private fun nameFromDisposition(header: String): String? {
        extendedName(header)?.let { return it }

        val match = Regex("""filename\s*=\s*(?:"([^"]*)"|([^;]*))""", RegexOption.IGNORE_CASE)
            .find(header) ?: return null
        val quoted = match.groupValues[1]
        val bare = match.groupValues[2]
        return quoted.ifEmpty { bare }.trim().ifEmpty { null }
    }

    /** `filename*=UTF-8''%E2%82%AC%20rates.pdf` — RFC 5987's encoded form. */
    private fun extendedName(header: String): String? {
        val match = Regex("""filename\*\s*=\s*([^';]*)'([^';]*)'([^;]*)""", RegexOption.IGNORE_CASE)
            .find(header) ?: return null
        val charset = match.groupValues[1].trim().ifEmpty { "UTF-8" }
        val encoded = match.groupValues[3].trim()
        if (encoded.isEmpty()) return null
        return percentDecode(encoded, charset)
    }

    /**
     * Decoded by hand rather than with `URLDecoder`, which is for form bodies and
     * turns `+` into a space — so `c++notes.txt` would arrive as `c  notes.txt`.
     */
    private fun percentDecode(text: String, charsetName: String): String? {
        val charset = try {
            Charset.forName(charsetName)
        } catch (_: Exception) {
            return null
        }
        val bytes = ByteArrayOutputStream(text.length)
        var i = 0
        while (i < text.length) {
            val char = text[i]
            val hex = if (char == '%' && i + 2 < text.length) {
                text.substring(i + 1, i + 3).toIntOrNull(16)
            } else {
                null
            }
            if (hex != null) {
                bytes.write(hex)
                i += 3
            } else {
                bytes.write(char.toString().toByteArray(charset))
                i++
            }
        }
        return String(bytes.toByteArray(), charset)
    }

    /** The last path segment of the URL, which is the usual source of a name. */
    private fun nameFromUrl(url: String): String? {
        val path = url.substringBefore('#').substringBefore('?')
        val last = path.substringAfterLast('/')
        if (last.isEmpty()) return null
        return percentDecode(last, "UTF-8") ?: last
    }

    /**
     * A name the file system cannot misread.
     *
     * Order matters here. Path separators go first, because everything after that
     * is about what a single segment may contain — strip the separators later and
     * `..%2Fx` decoded to `../x` would already have been treated as a name.
     */
    private fun sanitize(name: String?): String? {
        if (name == null) return null

        // Only the last segment was ever a file name. Both separators, because a
        // header written for Windows uses the other one and Android would keep it
        // as a literal character in the name.
        var out = name.substringAfterLast('/').substringAfterLast('\\')

        // Control characters, including the NUL byte that truncates a path in any
        // C library underneath, and the newlines that let one header line pretend
        // to be two.
        out = out.filter { it.code >= 0x20 && it.code != 0x7F }

        // Meaningful to some file system or shell somewhere, and never needed in
        // a name that came off the network.
        out = out.replace(Regex("""[:*?"<>|]"""), "_")

        // A leading dot makes a hidden file, and `.` and `..` are directories
        // rather than names; a trailing dot or space is silently dropped by some
        // file systems, which makes the saved name differ from the shown one.
        out = out.trimStart('.', ' ').trimEnd(' ', '.')

        return out.ifEmpty { null }
    }

    /** Adds an extension when the server named a type but not a suffix. */
    private fun withExtension(name: String, mimeType: String?): String {
        if (name.contains('.')) return name
        val type = mimeType?.substringBefore(';')?.trim()?.lowercase() ?: return name
        val extension = EXTENSIONS[type] ?: return name
        return "$name.$extension"
    }

    /** Truncates a long name without losing what it is. */
    private fun clamp(name: String): String {
        if (name.length <= MAX_LENGTH) return name
        val dot = name.lastIndexOf('.')
        val extension = if (dot > 0 && name.length - dot <= 12) name.substring(dot) else ""
        return name.take(MAX_LENGTH - extension.length) + extension
    }

    /**
     * Enough of the common web to be useful. Deliberately a fixed table and not
     * `MimeTypeMap`, so this file stays testable off a device.
     */
    private val EXTENSIONS = mapOf(
        "text/html" to "html",
        "text/plain" to "txt",
        "text/css" to "css",
        "text/csv" to "csv",
        "text/calendar" to "ics",
        "application/json" to "json",
        "application/xml" to "xml",
        "application/pdf" to "pdf",
        "application/zip" to "zip",
        "application/gzip" to "gz",
        "application/epub+zip" to "epub",
        "application/rtf" to "rtf",
        "application/vnd.android.package-archive" to "apk",
        "image/png" to "png",
        "image/jpeg" to "jpg",
        "image/gif" to "gif",
        "image/webp" to "webp",
        "image/svg+xml" to "svg",
        "audio/mpeg" to "mp3",
        "audio/ogg" to "ogg",
        "video/mp4" to "mp4",
        "video/webm" to "webm",
    )
}

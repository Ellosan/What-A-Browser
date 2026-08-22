package com.whatabrowser.wat.webview

/**
 * The diagnostic text a failed Tor window offers to copy.
 *
 * It exists because of a mistake in 0.1.3: private windows are `FLAG_SECURE`, so
 * when the Tor window failed there was no way to screenshot what it said. A
 * message you cannot get off the phone is a message nobody can act on.
 *
 * Rendering is kept here, away from Android, so that the two properties that
 * matter can be tested: it is bounded, and it contains only what it was given.
 * Nothing about which pages were open goes anywhere near it — that is the whole
 * point of the window it comes from.
 */
object TorReport {

    /**
     * Long enough for a reason to survive.
     *
     * This was 200, and the first real failure it reported was a library saying
     * exactly what the app was missing — cut off mid-sentence at the useful part.
     */
    private const val MAX_VALUE = 700

    private const val MAX_TOTAL = 4000

    fun render(fields: List<Pair<String, String>>, log: List<String>): String {
        val out = StringBuilder()
        out.append("WAT Tor diagnostics\n")
        for ((name, value) in fields) {
            out.append(name).append(": ").append(clamp(value)).append('\n')
        }
        out.append("\nrecent tor events:\n")
        if (log.isEmpty()) {
            out.append("  (none)\n")
        } else {
            for (line in log) out.append("  ").append(clamp(line)).append('\n')
        }

        // A report has to fit in a message someone will actually send. The oldest
        // events are the first to go, since the failure is at the end.
        if (out.length <= MAX_TOTAL) return out.toString()
        return out.substring(0, MAX_TOTAL - TRIMMED.length) + TRIMMED
    }

    private fun clamp(value: String): String {
        val single = value.replace('\n', ' ').trim()
        return if (single.length <= MAX_VALUE) single else single.take(MAX_VALUE - 1) + "…"
    }

    private const val TRIMMED = "\n… (trimmed)\n"
}

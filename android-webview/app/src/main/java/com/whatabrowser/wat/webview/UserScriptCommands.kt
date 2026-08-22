package com.whatabrowser.wat.webview

/**
 * The bridge between a script's registered menu commands and the browser's menu.
 *
 * `GM_registerMenuCommand` exists because a userscript manager is an extension
 * with a toolbar to hang things off. This browser has neither, so the shim in
 * `userscript-prelude.js` records each command on the page's own window and this
 * reads the list back out — which is what makes a script's own settings panel
 * reachable at all. Nearly every script that registers a command registers
 * exactly one, and it opens its settings.
 *
 * The traffic goes one way. `evaluateJavascript` is the app asking the page a
 * question; there is still no JavaScript interface anywhere in this browser, so
 * nothing in a page can start a conversation.
 *
 * The reply arrives as a JSON string literal — that is what `evaluateJavascript`
 * hands its callback — wrapping a list the page joined with two control
 * characters. Control characters rather than nested JSON because the answer has
 * to be read without a JSON library, and because the page strips both from every
 * caption before joining: the separators are then bytes the text cannot contain,
 * so the split cannot be fooled by what a caption says.
 */
object UserScriptCommands {

    class Command(val index: Int, val caption: String, val script: String)

    /** Separates the fields of one command. */
    private const val FIELD = '\u0000'

    /** Separates one command from the next. */
    private const val RECORD = '\u0001'

    /**
     * JavaScript that answers with the commands this document's scripts
     * registered, in the order they registered them.
     */
    val LIST: String = """
        (function () {
            var menu = window.__watUserscriptMenu || [];
            var strip = /[\u0000\u0001]/g;
            var out = [];
            for (var i = 0; i < menu.length; i++) {
                var entry = menu[i] || {};
                var caption = String(entry.caption == null ? '' : entry.caption).replace(strip, ' ');
                var script = String(entry.script == null ? '' : entry.script).replace(strip, ' ');
                out.push(i + '\u0000' + script + '\u0000' + caption);
            }
            return out.join('\u0001');
        })()
    """.trimIndent()

    /**
     * JavaScript that runs one of them.
     *
     * By position in the page's own list, taken from the same read that built the
     * menu. A script that registers another command in between cannot shift what
     * runs: the list only ever grows at the end, and a caption registered twice
     * replaces its own entry rather than appending a second one.
     */
    fun invoke(index: Int): String = """
        (function () {
            var menu = window.__watUserscriptMenu || [];
            var entry = menu[$index];
            if (!entry || typeof entry.action !== 'function') return 'gone';
            try {
                entry.action();
                return 'ok';
            } catch (error) {
                return 'failed';
            }
        })()
    """.trimIndent()

    /** Reads the reply to [LIST]. An answer that cannot be read is no commands. */
    fun parse(reply: String?): List<Command> {
        val decoded = decode(reply ?: return emptyList()) ?: return emptyList()
        if (decoded.isEmpty()) return emptyList()
        return decoded.split(RECORD).mapNotNull { record ->
            val fields = record.split(FIELD)
            if (fields.size != 3) return@mapNotNull null
            val index = fields[0].toIntOrNull() ?: return@mapNotNull null
            if (index < 0) return@mapNotNull null
            val caption = fields[2].trim()
            if (caption.isEmpty()) return@mapNotNull null
            Command(index, caption, fields[1].trim())
        }
    }

    /**
     * Unwraps the JSON string literal `evaluateJavascript` replies with.
     *
     * Written out rather than reached for through a JSON library, because this is
     * one shape — a single string — and because the same code then runs in a unit
     * test, where `org.json` is a stub that throws. Anything that is not one
     * string literal reads as null, which is what a page answering something
     * unexpected looks like from here.
     */
    fun decode(reply: String): String? {
        val trimmed = reply.trim()
        if (trimmed.length < 2 || !trimmed.startsWith('"') || !trimmed.endsWith('"')) return null
        val body = trimmed.substring(1, trimmed.length - 1)
        val out = StringBuilder(body.length)
        var index = 0
        while (index < body.length) {
            val char = body[index]
            if (char != '\\') {
                // A bare quote in the body means this was never one literal.
                if (char == '"') return null
                out.append(char)
                index++
                continue
            }
            index++
            if (index >= body.length) return null
            when (val escape = body[index]) {
                '"', '\\', '/' -> out.append(escape)
                'b' -> out.append('\b')
                'f' -> out.append('\u000C')
                'n' -> out.append('\n')
                'r' -> out.append('\r')
                't' -> out.append('\t')
                'u' -> {
                    if (index + 4 >= body.length) return null
                    val code = body.substring(index + 1, index + 5).toIntOrNull(16) ?: return null
                    out.append(code.toChar())
                    index += 4
                }
                else -> return null
            }
            index++
        }
        return out.toString()
    }
}

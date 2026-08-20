package com.whatabrowser.wat.webview

/**
 * How private this window is.
 *
 * Three modes rather than a private-tab flag, because the difference between them
 * is which *process* the pages are rendered in. Android's WebView has one cookie
 * and storage jar per process, so a private tab living beside a normal one would
 * share its cookies and be private in name only. Each private window is a second
 * process with a data directory of its own, thrown away when the window closes.
 */
enum class PrivacyMode {

    /** The ordinary browser: history, bookmarks, cookies that persist. */
    NORMAL,

    /** Hiding cat: nothing written down, own storage, thrown away on close. */
    CAT,

    /** Hiding lion: hiding cat, and every request through Tor. */
    LION,

    ;

    val isPrivate: Boolean get() = this != NORMAL

    /** Whether visits are recorded at all. */
    val recordsHistory: Boolean get() = this == NORMAL

    /** Whether the address bar may suggest from what has been visited before. */
    val suggestsFromHistory: Boolean get() = this == NORMAL

    val usesTor: Boolean get() = this == LION

    /** The data directory suffix, which is what gives a mode its own cookie jar. */
    val storageSuffix: String
        get() = when (this) {
            NORMAL -> "wat"
            CAT -> "cat"
            LION -> "lion"
        }

    companion object {
        fun of(name: String?): PrivacyMode =
            entries.firstOrNull { it.name == name } ?: NORMAL
    }
}

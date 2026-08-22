package com.whatabrowser.wat.webview

import android.app.Application
import android.os.Build
import android.util.Log
import android.webkit.WebView
import androidx.startup.AppInitializer
import androidx.startup.Initializer
import androidx.annotation.RequiresApi

/**
 * Which storage jar this process gets.
 *
 * This is the whole mechanism behind private browsing here. Android's WebView
 * keeps one cookie and storage jar per *process*, and `setDataDirectorySuffix`
 * chooses which one — so a private window is not a flag on a tab, it is a second
 * process with a directory of its own that gets deleted. A private tab sharing
 * the ordinary process would share its cookies, and be private in name only.
 *
 * The suffix has to be set before any WebView is touched, which is why it happens
 * here and not in the activity.
 */
class BrowserApp : Application() {

    override fun onCreate() {
        super.onCreate()
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) {
            // One process, so it is the ordinary one by definition.
            seedBundledScripts()
            return
        }

        // Every process runs this, so the process name is what decides. The
        // private processes are declared in the manifest as `:cat` and `:lion`.
        val process = processName()
        val mode = when {
            process.endsWith(":cat") -> PrivacyMode.CAT
            process.endsWith(":lion") -> PrivacyMode.LION
            else -> PrivacyMode.NORMAL
        }
        WebView.setDataDirectorySuffix(mode.storageSuffix)

        // Only the ordinary process offers the bundled scripts. The record of
        // what has already been offered is one preferences file, and two
        // processes writing it is how a script someone deleted comes back.
        if (mode == PrivacyMode.NORMAL) seedBundledScripts()

        // Whatever the last private window left behind, before anything can open
        // it again. Done on the way in rather than on the way out because a
        // window that is killed — by the system, or by the reader swiping it
        // away — never gets to run its own cleanup.
        PrivateStorage.wipe(this, mode)

        if (mode.usesTor) registerTorLibraries()
    }

    /**
     * Puts the scripts that ship with the browser on disk, the first time.
     *
     * Off the main thread, because this reads the assets and writes a file and
     * neither belongs in a cold start. Nothing waits on it: a script is only
     * needed once a page loads, and the store is read fresh each time.
     */
    private fun seedBundledScripts() {
        Thread({
            runCatching { UserScriptStore(this).seedBundled(this) }
        }, "userscript-seed").apply {
            isDaemon = true
            start()
        }
    }

    /**
     * Tells the tor library where its native libraries are, in this process.
     *
     * kmp-tor registers them from an `androidx.startup` initializer, which runs
     * from a `ContentProvider` — and a `ContentProvider` is created only in the
     * process that hosts it, which is the main one. The Tor window is a process
     * of its own, precisely so that its cookies and its proxy are its own, and in
     * that process the initializer had never run: tor started, looked for
     * `libtor.so`, and reported it missing while the file sat in the app's
     * library directory all along.
     *
     * So it is initialised by hand here, where `Application.onCreate` runs in
     * every process. Only in the Tor process: the ordinary window never loads any
     * of this, which is what keeps its cold start the way it was.
     */
    private fun registerTorLibraries() {
        try {
            // By name because the class is `internal` to the library and cannot
            // be referred to directly from Kotlin. A name in a string is a name
            // that can rot, so `TorInitializerTest` fails the build if a future
            // version of kmp-tor moves it — rather than letting the Tor window
            // discover it on someone's phone.
            @Suppress("UNCHECKED_CAST")
            val initializer = Class.forName(TorEngine.RESOURCE_INITIALIZER) as Class<out Initializer<Any>>

            // By now [TorStartupProvider] has run: content providers are created
            // before `Application.onCreate`. Asking is all that is left, because
            // this particular initializer refuses to be started any way but
            // eagerly — which is what 0.1.5 got wrong by calling
            // `initializeComponent` and being told "cannot be initialized
            // lazily".
            val eager = AppInitializer.getInstance(this).isEagerlyInitialized(initializer)
            TorEngine.record("tor resource initializer eager=$eager")
            if (!eager) {
                TorEngine.record(
                    "the tor startup provider did not run in this process " +
                        "(${TorStartupProvider::class.java.name})",
                )
            }
        } catch (throwable: Throwable) {
            // Reported rather than thrown: the window's own gate will say tor
            // could not start, and this line says why in the diagnostics.
            TorEngine.record("tor resource initializer check FAILED: $throwable")
            Log.e(TAG, "tor resource initializer check failed", throwable)
        }
    }

    // Only ever reached from behind the API 28 check above; the annotation is
    // what lets lint see that.
    @RequiresApi(Build.VERSION_CODES.P)
    private fun processName(): String = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        getProcessName()
    } else {
        @Suppress("DEPRECATION")
        Application.getProcessName()
    }

    private companion object {
        const val TAG = "wat-tor"
    }
}

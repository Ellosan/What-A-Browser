package com.whatabrowser.wat.webview

import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.util.Log
import io.matthewnelson.kmp.tor.resource.noexec.tor.ResourceLoaderTorNoExec
import io.matthewnelson.kmp.tor.runtime.Action
import io.matthewnelson.kmp.tor.runtime.RuntimeEvent
import io.matthewnelson.kmp.tor.runtime.TorRuntime
import java.io.File

/**
 * Tor, inside the app.
 *
 * Until 0.1.3 the Tor window needed Orbot installed, which is a fair thing to ask
 * of someone who already knows what Orbot is and no thing at all to ask of
 * everyone else. Desktop Brave does not ask; it carries Tor. So does this now:
 * `libtor.so` ships in the APK and is loaded into the lion window's process, and
 * there is nothing else to install.
 *
 * It is the real tor, from the kmp-tor project's build of the tor source, loaded
 * as a library rather than executed as a binary — Android blocks executing files
 * outside the app's native library directory, and this side-steps that whole
 * argument.
 *
 * Nothing here is touched by the ordinary window. The classes are only loaded in
 * the `:lion` process, so a browser that never opens a Tor window never pays for
 * one — which is what keeps the cold start this build exists for.
 */
object TorEngine {

    sealed interface State {
        /** Bootstrapping, [percent] of the way through building a circuit. */
        class Starting(val percent: Int) : State

        /** Running, with a local HTTP proxy at [proxy] that goes through Tor. */
        class Ready(val proxy: String) : State

        class Failed(val reason: String) : State
    }

    private const val TAG = "wat-tor"

    /**
     * kmp-tor's androidx.startup initializer, which registers where the native
     * libraries are.
     *
     * Held here as a name because the class is `internal` to the library. It is
     * checked by a test at build time, so a version of kmp-tor that moves it
     * fails here rather than on a phone.
     */
    const val RESOURCE_INITIALIZER =
        "io.matthewnelson.kmp.tor.resource.compilation.lib.tor.KmpTorResourceInitializer"

    private val main = Handler(Looper.getMainLooper())

    private var runtime: TorRuntime? = null
    private var bridge: TorBridge? = null
    private var listener: ((State) -> Unit)? = null

    /** The last few things tor said, for the report a failure offers to copy. */
    val log = TorLog()

    private var ready = false

    /**
     * Everything worth knowing when it does not work.
     *
     * A Tor window that fails is the hardest thing in this browser to diagnose:
     * it cannot be screenshotted, it is in its own process, and the failure is
     * usually in a layer nobody can see. So the reasons are collected as they
     * happen rather than reconstructed afterwards.
     */
    fun diagnostics(context: Context, reason: String): String = TorReport.render(
        listOf(
            "wat" to BuildConfig.VERSION_NAME,
            "reason" to reason,
            "android" to "${Build.VERSION.RELEASE} (API ${Build.VERSION.SDK_INT})",
            "device" to "${Build.MANUFACTURER} ${Build.MODEL}",
            "abis" to Build.SUPPORTED_ABIS.joinToString(", "),
            "tor library" to describeLibrary(context),
            "webview proxy support" to TorGate.isSupported().toString(),
            "socks" to (socksAddress ?: "none"),
            "bridge" to (bridgeAddress ?: "none"),
        ),
        log.lines(),
    )

    private var socksAddress: String? = null
    private var bridgeAddress: String? = null

    /**
     * Whether this device's architecture is one the installed APK carries tor
     * for.
     *
     * This is the failure worth naming precisely, because it has nothing to do
     * with tor: the build ships one APK per architecture, and an APK for the
     * wrong one has no `libtor.so` in it at all. `System.loadLibrary` would say
     * only that it could not find something.
     */
    private fun describeLibrary(context: Context): String {
        val directory = java.io.File(context.applicationInfo.nativeLibraryDir)
        val library = java.io.File(directory, "libtor.so")
        return if (library.isFile) {
            "${library.length() / 1024} KB at ${directory.name}"
        } else {
            "MISSING from ${directory.path} — this APK has no tor for this device"
        }
    }

    fun hasLibrary(context: Context): Boolean =
        java.io.File(context.applicationInfo.nativeLibraryDir, "libtor.so").isFile

    /**
     * Starts tor, then the bridge in front of it, reporting progress as it goes.
     *
     * The callback always arrives on the main thread: tor's own events come off
     * its executor, and an interface updated from there is a crash waiting for a
     * slow phone.
     */
    fun start(context: Context, onState: (State) -> Unit) {
        listener = onState

        // Checked before anything else, and named plainly: an APK built for
        // another architecture is the one failure here that no amount of
        // retrying fixes.
        if (!hasLibrary(context)) {
            note("no libtor.so in ${context.applicationInfo.nativeLibraryDir}")
            report(
                State.Failed(
                    "this build has no tor for this device (" +
                        Build.SUPPORTED_ABIS.joinToString(", ") +
                        "). Install the universal APK, or the one for this architecture.",
                ),
            )
            return
        }

        val existing = runtime
        if (existing != null) {
            // Already up from an earlier attempt in this process; hand back the
            // proxy rather than starting a second tor.
            note("tor is already running in this process")
            val socks = existing.listeners().socks.firstOrNull()
            if (socks != null) {
                publish(socks)
            } else {
                // Still coming up. The observers on the runtime will report when
                // it does, so there is nothing to do but say so.
                report(State.Starting(50))
            }
            return
        }

        note("starting tor")
        report(State.Starting(0))
        try {
            val work = File(context.filesDir, "tor").apply { mkdirs() }
            val cache = File(context.cacheDir, "tor").apply { mkdirs() }

            val environment = TorRuntime.Environment.Builder(work, cache) { resourceDir ->
                ResourceLoaderTorNoExec.getOrCreate(resourceDir)
            }

            val built = TorRuntime.Builder(environment) {
                observerStatic(RuntimeEvent.STATE) { state ->
                    val percent = state.daemon.bootstrap.toInt() and 0xFF
                    note("state ${state.daemon} network=${state.network}")
                    if (!state.daemon.isBootstrapped) report(State.Starting(percent))
                }
                observerStatic(RuntimeEvent.ERROR) { throwable ->
                    note("error ${describe(throwable)}")
                    // An error after the window is up is worth recording but must
                    // not tear down a session that is working.
                    if (!ready) report(State.Failed(describe(throwable)))
                }
                observerStatic(RuntimeEvent.LOG.WARN) { line -> note("warn $line") }
                observerStatic(RuntimeEvent.LISTENERS) { listeners ->
                    // Whichever arrives first — this or READY — is what opens the
                    // window. At READY the SOCKS port has usually been announced
                    // already, but "usually" is not something to build a gate on:
                    // if it has not, waiting for this is the difference between a
                    // Tor window that opens and one that says tor has no SOCKS
                    // port and gives up.
                    note("listeners socks=${listeners.socks}")
                    if (!ready) listeners.socks.firstOrNull()?.let { publish(it) }
                }
                observerStatic(RuntimeEvent.READY) { _ ->
                    note("ready")
                    if (!ready) {
                        val socks = runtime?.listeners()?.socks?.firstOrNull()
                        if (socks == null) {
                            note("ready with no SOCKS listener yet; waiting for one")
                        } else {
                            publish(socks)
                        }
                    }
                }
            }
            runtime = built

            built.enqueue(
                Action.StartDaemon,
                { throwable ->
                    note("start failed ${describe(throwable)}")
                    report(State.Failed(describe(throwable)))
                },
                { note("start accepted") },
            )
        } catch (throwable: Throwable) {
            // A device that cannot write its data directory lands here, as does
            // one where the library is present but will not load.
            note("could not build a runtime: ${describe(throwable)}")
            report(State.Failed(describe(throwable)))
        }
    }

    /**
     * Puts the bridge in front of tor's SOCKS port and announces the address.
     */
    private fun publish(socks: io.matthewnelson.kmp.tor.runtime.core.net.IPSocketAddress) {
        socksAddress = "${socks.address.value}:${socks.port.value}"
        note("socks on $socksAddress")

        val started = bridge ?: TorBridge(socks.address.value, socks.port.value)
        val port = started.start()
        if (port == 0) {
            note("the local bridge would not bind")
            report(State.Failed("the local proxy could not be opened"))
            return
        }
        bridge = started
        bridgeAddress = "127.0.0.1:$port"
        ready = true
        note("bridge on $bridgeAddress")
        report(State.Ready("127.0.0.1:$port"))
    }

    /**
     * A throwable as one line, with its causes.
     *
     * `message` alone is very often null — which is how 0.1.3's Tor failure came
     * to show a reason nobody could act on.
     */
    private fun describe(throwable: Throwable): String {
        val parts = mutableListOf<String>()
        var current: Throwable? = throwable
        var depth = 0
        while (current != null && depth++ < 4) {
            val name = current.javaClass.simpleName
            val message = current.message
            parts.add(if (message.isNullOrBlank()) name else "$name: $message")
            current = current.cause
        }
        throwable.stackTrace.firstOrNull()?.let { parts.add("at $it") }
        return parts.joinToString(" <- ")
    }

    /** For the parts of startup that happen before this object is touched. */
    fun record(line: String) = note(line)

    private fun note(line: String) {
        log.add(line)
        // Also to logcat, so `adb logcat -s wat-tor` works for anyone with a
        // computer to hand.
        Log.i(TAG, line)
    }

    fun stop() {
        note("stopping")
        ready = false
        socksAddress = null
        bridgeAddress = null
        bridge?.stop()
        bridge = null
        runtime?.enqueue(Action.StopDaemon, {}, {})
        runtime = null
        listener = null
    }

    private fun report(state: State) {
        val to = listener ?: return
        main.post { to(state) }
    }
}

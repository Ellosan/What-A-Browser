package com.whatabrowser.wat.webview

import android.content.Context
import android.os.Handler
import android.os.Looper
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

    private val main = Handler(Looper.getMainLooper())

    private var runtime: TorRuntime? = null
    private var bridge: TorBridge? = null
    private var listener: ((State) -> Unit)? = null

    /**
     * Starts tor, then the bridge in front of it, reporting progress as it goes.
     *
     * The callback always arrives on the main thread: tor's own events come off
     * its executor, and an interface updated from there is a crash waiting for a
     * slow phone.
     */
    fun start(context: Context, onState: (State) -> Unit) {
        listener = onState
        val existing = runtime
        if (existing != null) {
            // Already up from an earlier attempt in this process; hand back the
            // proxy rather than starting a second tor.
            bridge?.let { report(State.Starting(100)) }
            publishReady(existing)
            return
        }

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
                    if (!state.daemon.isBootstrapped) report(State.Starting(percent))
                }
                observerStatic(RuntimeEvent.ERROR) { throwable ->
                    report(State.Failed(throwable.message ?: throwable.toString()))
                }
                observerStatic(RuntimeEvent.READY) { _ ->
                    runtime?.let(::publishReady)
                }
            }
            runtime = built

            built.enqueue(
                Action.StartDaemon,
                { throwable -> report(State.Failed(throwable.message ?: "tor did not start")) },
                { },
            )
        } catch (throwable: Throwable) {
            // A device whose ABI has no tor in this APK lands here, as does one
            // that cannot write its data directory.
            report(State.Failed(throwable.message ?: "tor could not be loaded"))
        }
    }

    /**
     * Puts the bridge in front of tor's SOCKS port and announces the address.
     */
    private fun publishReady(built: TorRuntime) {
        val socks = built.listeners().socks.firstOrNull()
        if (socks == null) {
            report(State.Failed("tor opened no SOCKS port"))
            return
        }

        val started = bridge ?: TorBridge(socks.address.value, socks.port.value)
        val port = started.start()
        if (port == 0) {
            report(State.Failed("the local proxy could not be opened"))
            return
        }
        bridge = started
        report(State.Ready("127.0.0.1:$port"))
    }

    fun stop() {
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

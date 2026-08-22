package com.whatabrowser.wat.webview

import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/**
 * The HTTP proxy the WebView talks to, which is really tor's SOCKS port.
 *
 * Android's `ProxyController` — the only way to point a WebView at a proxy —
 * speaks HTTP proxies and nothing else. Tor speaks SOCKS5. This is the fifty
 * lines of plumbing that joins them: it accepts `CONNECT host:port` on loopback,
 * opens a SOCKS5 tunnel through tor to the same place, answers `200`, and then
 * copies bytes in both directions without looking at them.
 *
 * Two properties worth stating, since this is what all Tor traffic goes through:
 *
 * - **It only tunnels.** [HttpConnect] refuses anything that is not a CONNECT, so
 *   the bridge cannot be talked into fetching a URL for anyone.
 * - **It never resolves a name.** The host is handed to tor as a name, so the
 *   lookup happens inside the circuit. Resolving here would send a DNS query out
 *   of the phone in the clear, naming every site visited — traffic inside Tor,
 *   lookups outside it.
 *
 * It binds to loopback, so it is reachable only from this device; while a lion
 * window is open, another app on the phone could use it as a proxy. It is also
 * only alive while that window is.
 */
class TorBridge(private val socksHost: String, private val socksPort: Int) {

    private val running = AtomicBoolean(false)
    private var server: ServerSocket? = null

    private val threads = Executors.newCachedThreadPool { runnable ->
        Thread(runnable, "tor-bridge").apply { isDaemon = true }
    }

    /** Starts listening, and returns the local port, or 0 if it could not. */
    fun start(): Int {
        if (running.get()) return server?.localPort ?: 0
        return try {
            val socket = ServerSocket(0, BACKLOG, InetAddress.getByName(LOOPBACK))
            server = socket
            running.set(true)
            threads.execute { accept(socket) }
            socket.localPort
        } catch (_: IOException) {
            0
        }
    }

    fun stop() {
        running.set(false)
        runCatching { server?.close() }
        server = null
        threads.shutdownNow()
    }

    private fun accept(socket: ServerSocket) {
        while (running.get() && !socket.isClosed) {
            val client = try {
                socket.accept()
            } catch (_: IOException) {
                return // closed, or the listener died with the window
            }
            threads.execute { serve(client) }
        }
    }

    private fun serve(client: Socket) {
        var remote: Socket? = null
        try {
            client.soTimeout = HANDSHAKE_TIMEOUT_MS
            val input = client.getInputStream()
            val output = client.getOutputStream()

            val requestLine = readLine(input) ?: return
            drainHeaders(input)

            val target = HttpConnect.parse(requestLine)
            if (target == null) {
                output.write(HttpConnect.NOT_ALLOWED.toByteArray())
                output.flush()
                return
            }

            remote = openTunnel(target)
            if (remote == null) {
                output.write(HttpConnect.refusal("tor refused the tunnel").toByteArray())
                output.flush()
                return
            }

            output.write(HttpConnect.ESTABLISHED.toByteArray())
            output.flush()

            // From here the bridge is a pipe. Both directions run at once, and
            // the timeout comes off: a tunnel is allowed to be idle.
            client.soTimeout = 0
            remote.soTimeout = 0
            val far = remote
            threads.execute { pump(input, far.getOutputStream()) }
            pump(far.getInputStream(), output)
        } catch (_: Exception) {
            // A client that hung up mid-handshake is ordinary, not exceptional.
        } finally {
            runCatching { remote?.close() }
            runCatching { client.close() }
        }
    }

    /** The SOCKS5 handshake, in the order tor expects it. */
    private fun openTunnel(target: HttpConnect.Target): Socket? {
        val request = Socks5.connect(target.host, target.port) ?: return null
        val socket = Socket()
        return try {
            socket.connect(InetSocketAddress(socksHost, socksPort), CONNECT_TIMEOUT_MS)
            socket.soTimeout = HANDSHAKE_TIMEOUT_MS
            val input = socket.getInputStream()
            val output = socket.getOutputStream()

            output.write(Socks5.greeting())
            output.flush()
            val greeting = ByteArray(2)
            if (!readFully(input, greeting) || !Socks5.greetingAccepted(greeting)) {
                socket.close()
                return null
            }

            output.write(request)
            output.flush()
            val header = ByteArray(4)
            if (!readFully(input, header) || Socks5.status(header) != Socks5.REPLY_OK) {
                socket.close()
                return null
            }

            // The reply carries the address tor bound, which has to be read off
            // the socket before the tunnel's own bytes begin.
            val addressType = header[3].toInt() and 0xFF
            val firstByte = if (addressType == Socks5.ADDRESS_NAME.toInt()) {
                val length = ByteArray(1)
                if (!readFully(input, length)) {
                    socket.close()
                    return null
                }
                length[0].toInt() and 0xFF
            } else {
                0
            }
            val remaining = Socks5.addressLength(addressType, firstByte) -
                if (addressType == Socks5.ADDRESS_NAME.toInt()) 1 else 0
            if (remaining < 0 || !readFully(input, ByteArray(remaining))) {
                socket.close()
                return null
            }
            socket
        } catch (_: Exception) {
            runCatching { socket.close() }
            null
        }
    }

    private fun pump(from: InputStream, to: OutputStream) {
        val buffer = ByteArray(BUFFER)
        try {
            while (true) {
                val read = from.read(buffer)
                if (read < 0) break
                to.write(buffer, 0, read)
                to.flush()
            }
        } catch (_: Exception) {
            // Either end closing is how a tunnel ends.
        } finally {
            runCatching { to.close() }
        }
    }

    /** One CRLF-terminated line, refusing to grow past what a request line may be. */
    private fun readLine(input: InputStream): String? {
        val out = StringBuilder()
        while (out.length <= HttpConnect.MAX_LINE) {
            val byte = input.read()
            if (byte < 0) return null
            if (byte == '\n'.code) return out.toString().trimEnd('\r')
            out.append(byte.toChar())
        }
        return null
    }

    private fun drainHeaders(input: InputStream) {
        var lines = 0
        while (lines++ < MAX_HEADERS) {
            val line = readLine(input) ?: return
            if (line.isEmpty()) return
        }
    }

    private fun readFully(input: InputStream, into: ByteArray): Boolean {
        var offset = 0
        while (offset < into.size) {
            val read = input.read(into, offset, into.size - offset)
            if (read < 0) return false
            offset += read
        }
        return true
    }

    private companion object {
        const val LOOPBACK = "127.0.0.1"
        const val BACKLOG = 32
        const val BUFFER = 8 * 1024
        const val MAX_HEADERS = 64
        const val CONNECT_TIMEOUT_MS = 20_000
        const val HANDSHAKE_TIMEOUT_MS = 30_000
    }
}

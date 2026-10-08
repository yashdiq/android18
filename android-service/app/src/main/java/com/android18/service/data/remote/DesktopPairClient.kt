package com.android18.service.data.remote

import com.android18.service.data.server.dto.PairingQrDto
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import java.io.IOException
import java.net.HttpURLConnection
import java.net.URL
import javax.inject.Inject

/** The QR the desktop pairing sheet shows. */
@Serializable
data class DesktopQrPayload(
    val v: Int = 1,
    val ip: String = "",
    val port: Int = 0,
    val code: String = "",
    /** Legacy/fallback single-endpoint form; split on first ':'. */
    val endpoint: String = "",
) {
    val isUsable: Boolean
        get() = (ip.isNotBlank() && port in 1..65535) ||
            endpoint.substringAfterLast(':').toIntOrNull() in 1..65535
}

/** Outcome of posting our identity to the desktop's pairing listener. */
sealed interface PairOutcome {
    /** HTTP 200 — the desktop accepted the one-time code. */
    data object Paired : PairOutcome

    /** 4xx — wrong/expired code, or the desktop rejected the payload. */
    data class Rejected(val status: Int, val body: String) : PairOutcome

    /** Network failure — desktop unreachable, timeout, bad payload. */
    data class Error(val message: String) : PairOutcome
}

/**
 * Posts the phone's pairing identity to the desktop's one-shot listener
 * (`POST /pair`, one-time code in `X-Pair-Code`). Plain
 * [HttpURLConnection] on [Dispatchers.IO] — the call is short-lived and
 * single-purpose, Ktor is not spun up for it.
 */
class DesktopPairClient @Inject constructor() {

    private val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = true
        isLenient = true
    }

    fun parseQrPayload(raw: String): DesktopQrPayload? {
        val trimmed = raw.trim()
        // Compact pipe format: `a18|<v>|<ip>|<port>|<code>` — QR v3 instead
        // of the legacy JSON's v5, which is what made scanning slow. v2
        // carries a 6-char code, v1 the old 12-hex one; both parse alike.
        if (trimmed.startsWith("a18|")) {
            val parts = trimmed.split('|')
            if (parts.size == 5 && (parts[1] == "1" || parts[1] == "2")) {
                // Bracketed IPv6 literals (if a desktop ever ships one)
                // strip to the bare address the URL layer re-brackets.
                val ip = parts[2].removePrefix("[").removeSuffix("]")
                val port = parts[3].toIntOrNull() ?: 0
                val code = parts[4]
                if (ip.isNotBlank() && port in 1..65535 && code.isNotBlank()) {
                    return DesktopQrPayload(ip = ip, port = port, code = code)
                }
            }
            return null
        }
        // Legacy JSON form (older desktops still ship it).
        return runCatching { json.decodeFromString<DesktopQrPayload>(trimmed) }.getOrNull()
            ?.takeIf { it.isUsable }
    }

    suspend fun pair(desktop: DesktopQrPayload, self: PairingQrDto): PairOutcome =
        withContext(Dispatchers.IO) {
            val host = desktop.ip.ifBlank { desktop.endpoint.substringBeforeLast(':') }
            val port = if (desktop.port in 1..65535) {
                desktop.port
            } else {
                desktop.endpoint.substringAfterLast(':').toIntOrNull() ?: 0
            }
            if (host.isBlank() || port !in 1..65535) {
                return@withContext PairOutcome.Error("QR has no usable endpoint")
            }
            // IPv6 literals need brackets in URLs; IPv4 and hostnames pass.
            val urlHost = if (host.contains(':')) "[$host]" else host
            // Tell the desktop the address that actually routes to it, not
            // whichever interface the phone happens to list first.
            val routed = routeAddressTo(host, port)
            val body = json.encodeToString(if (routed != null) self.copy(ip = routed) else self)
            var connection: HttpURLConnection? = null
            try {
                connection = (URL("http://$urlHost:$port/pair").openConnection() as HttpURLConnection)
                    .apply {
                        requestMethod = "POST"
                        connectTimeout = TIMEOUT_MS
                        readTimeout = TIMEOUT_MS
                        doOutput = true
                        setRequestProperty("Content-Type", "application/json")
                        setRequestProperty("X-Pair-Code", desktop.code)
                        setRequestProperty("Connection", "close")
                    }
                connection.outputStream.use { it.write(body.toByteArray()) }
                val status = connection.responseCode
                when {
                    status in 200..299 -> PairOutcome.Paired
                    status in 400..499 -> PairOutcome.Rejected(status, connection.errorStream?.bufferedReader()?.use { it.readText() }.orEmpty())
                    else -> PairOutcome.Error("desktop answered HTTP $status")
                }
            } catch (e: IOException) {
                PairOutcome.Error(e.message ?: e.javaClass.simpleName)
            } finally {
                connection?.disconnect()
            }
        }

    /**
     * Local address the OS would use to reach [host] — a UDP `connect()`
     * picks the route without sending a packet. Null when unroutable.
     */
    private fun routeAddressTo(host: String, port: Int): String? = runCatching {
        java.net.DatagramSocket().use { socket ->
            socket.connect(java.net.InetAddress.getByName(host), port)
            socket.localAddress.hostAddress?.takeUnless { it == "0.0.0.0" || it.startsWith("127.") }
        }
    }.getOrNull()

    private companion object {
        const val TIMEOUT_MS = 5_000
    }
}

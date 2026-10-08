package com.android18.service

import com.android18.service.data.remote.DesktopPairClient
import com.android18.service.data.remote.DesktopQrPayload
import com.android18.service.data.remote.PairOutcome
import com.android18.service.data.server.dto.PairingQrDto
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.net.ServerSocket

class DesktopPairClientTest {

    private val client = DesktopPairClient()
    private val self = PairingQrDto(
        name = "Pixel 9 Pro",
        ip = "192.168.1.42",
        port = 8080,
        token = "7f9c2d1b84e0",
        deviceId = "android-abc",
    )

    @Test
    fun parsesCompactV2AndLegacyV1PipePayloads() {
        val v2 = client.parseQrPayload("a18|2|192.168.1.10|51234|0X1D8C")
        assertNotNull(v2)
        assertEquals("192.168.1.10", v2!!.ip)
        assertEquals(51234, v2.port)
        assertEquals("0X1D8C", v2.code)
        assertEquals("a1b2c3d4e5f6", client.parseQrPayload("a18|1|10.0.0.2|8080|a1b2c3d4e5f6")!!.code)
        assertEquals(null, client.parseQrPayload("a18|3|10.0.0.2|8080|0X1D8C"))
    }

    @Test
    fun parsesDesktopQrPayload() {
        val payload = client.parseQrPayload(
            """{"v":1,"ip":"192.168.1.10","port":51234,"code":"a1b2c3d4e5f6"}""",
        )
        assertNotNull(payload)
        assertEquals("192.168.1.10", payload!!.ip)
        assertEquals(51234, payload.port)
        assertEquals("a1b2c3d4e5f6", payload.code)
        assertTrue(payload.isUsable)
    }

    @Test
    fun toleratesExtraFieldsAndEndpointForm() {
        val payload = client.parseQrPayload(
            """{"v":1,"endpoint":"192.168.1.10:51234","code":"a1b2c3d4e5f6","extra":"x"}""",
        )
        assertNotNull(payload)
        assertTrue(payload!!.isUsable)
    }

    @Test
    fun rejectsGarbageAndUnusableCodes() {
        assertNull(client.parseQrPayload("not json at all"))
        assertNull(client.parseQrPayload("""{"v":1,"ip":"","port":0,"code":""}"""))
        assertNull(client.parseQrPayload("""{"v":1,"ip":"10.0.0.1","port":99999,"code":"x"}"""))
    }

    @Test
    fun endpointFallbackSplitsHostAndPort() {
        val fallback = DesktopQrPayload(endpoint = "192.168.1.9:41000")
        assertTrue(fallback.isUsable)
        assertFalse(DesktopQrPayload(ip = "192.168.1.9", port = 0).isUsable)
    }

    @Test
    fun stripsBracketsFromIpv6Literals() {
        val payload = client.parseQrPayload("a18|2|[fe80::1]|51234|0X1D8C")
        assertNotNull(payload)
        assertEquals("fe80::1", payload!!.ip)
        assertEquals(51234, payload.port)
    }

    @Test
    fun pairingToAnIpv6LiteralFormsAValidUrl() {
        // Bind on the IPv6 loopback, close it, and pair — a malformed
        // URL would crash before the connect could fail.
        val port = ServerSocket(0, 1, java.net.InetAddress.getByName("::1")).use { it.localPort }
        val outcome = runBlocking {
            client.pair(DesktopQrPayload(ip = "::1", port = port, code = "a1b2c3d4e5f6"), self)
        }
        assertTrue(outcome is PairOutcome.Error)
    }

    @Test
    fun pairingAgainstAClosedPortIsAnError() {
        // Bind an ephemeral port and close it again — connects fail fast.
        val port = ServerSocket(0).use { it.localPort }
        val outcome = runBlocking {
            client.pair(DesktopQrPayload(ip = "127.0.0.1", port = port, code = "a1b2c3d4e5f6"), self)
        }
        assertTrue(outcome is PairOutcome.Error)
    }

    @Test
    fun unusableQrShortCircuitsWithoutNetworking() {
        val outcome = runBlocking {
            client.pair(DesktopQrPayload(ip = "", port = 0, endpoint = ""), self)
        }
        assertTrue(outcome is PairOutcome.Error)
    }
}

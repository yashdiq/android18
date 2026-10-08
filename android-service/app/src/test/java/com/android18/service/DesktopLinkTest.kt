package com.android18.service

import com.android18.service.domain.model.JournalEntry
import com.android18.service.util.activeDesktop
import com.android18.service.util.pickLanIpv4
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class DesktopLinkTest {

    private fun entry(endpoint: String, status: Int, at: Long, client: String? = "192.168.1.5") =
        JournalEntry("GET", endpoint, status, client, at)

    @Test
    fun recentAuthorizedRequestMeansConnected() {
        val now = 100_000L
        assertEquals("192.168.1.5", activeDesktop(listOf(entry("/info", 200, now - 4_000)), now))
    }

    @Test
    fun staleFailedOrPairRequestEntriesDoNotCount() {
        val now = 100_000L
        assertNull(activeDesktop(listOf(entry("/info", 200, now - 20_000)), now))
        assertNull(activeDesktop(listOf(entry("/info", 401, now - 1_000)), now))
        assertNull(activeDesktop(listOf(entry("/pair-request", 200, now - 1_000)), now))
        assertNull(activeDesktop(emptyList(), now))
    }

    @Test
    fun lanPickerSkipsCellularAndVpnAndPrefersWifi() {
        val picked = pickLanIpv4(
            listOf("rmnet_data1" to "10.64.1.2", "tun0" to "10.8.0.2", "wlan0" to "192.168.1.20"),
        )
        assertEquals("192.168.1.20", picked)
        assertEquals("192.168.43.1", pickLanIpv4(listOf("rmnet0" to "10.1.1.1", "ap0" to "192.168.43.1")))
        assertEquals("172.16.0.9", pickLanIpv4(listOf("tun0" to "10.8.0.2", "foo0" to "172.16.0.9")))
        assertNull(pickLanIpv4(listOf("rmnet0" to "10.1.1.1", "tun0" to "10.8.0.2")))
    }
}

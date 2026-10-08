package com.android18.service.util

import java.net.Inet4Address
import java.net.NetworkInterface

/** Interfaces that never carry the Wi-Fi/LAN route to a desktop. */
private val NON_LAN_PREFIXES = listOf("rmnet", "ccmni", "tun", "ppp", "v4-", "dummy", "clat", "lo")

/** Interfaces that do, in preference order. */
private val LAN_PREFIXES = listOf("wlan", "swlan", "ap", "eth", "p2p")

/**
 * Picks the LAN address out of `(interface name, IPv4)` pairs: cellular
 * (`rmnet*`/`ccmni*`), VPN (`tun*`/`ppp*`) and CLAT interfaces are skipped,
 * and Wi-Fi/hotspot/Ethernet names win over anything unrecognized. The
 * first-interface-wins approach posted a cellular address to the desktop.
 */
fun pickLanIpv4(candidates: List<Pair<String, String>>): String? {
    val usable = candidates.filter { (name, _) -> NON_LAN_PREFIXES.none { name.startsWith(it) } }
    for (prefix in LAN_PREFIXES) {
        usable.firstOrNull { (name, _) -> name.startsWith(prefix) }?.let { return it.second }
    }
    return usable.firstOrNull()?.second
}

/** The phone's LAN IPv4, or null when off-Wi-Fi. */
fun lanIpv4(): String? =
    runCatching {
        pickLanIpv4(
            NetworkInterface.getNetworkInterfaces().asSequence()
                .filter { it.isUp && !it.isLoopback }
                .flatMap { nic ->
                    nic.inetAddresses.asSequence()
                        .filterIsInstance<Inet4Address>()
                        .filter { !it.isLoopbackAddress }
                        .map { nic.name to it.hostAddress.orEmpty() }
                }
                .filter { it.second.isNotEmpty() }
                .toList(),
        )
    }.getOrNull()

package com.android18.service.data.server

import android.os.Build
import java.net.InetAddress
import javax.jmdns.JmDNS
import javax.jmdns.ServiceInfo

/** Advertises `_android18._tcp.local.` with `id`/`name`/`model`/`pair` TXT records. */
object MdnsAdvertiser {

    const val SERVICE_TYPE = "_android18._tcp.local."

    private var jmdns: JmDNS? = null

    @Synchronized
    fun start(deviceId: String, port: Int) {
        if (jmdns != null) return
        Thread {
            runCatching {
                val address = InetAddress.getLocalHost()
                val service = ServiceInfo.create(
                    SERVICE_TYPE,
                    deviceId,
                    port,
                    0,
                    0,
                    mapOf(
                        "id" to deviceId,
                        "name" to (Build.MODEL ?: "Android"),
                        "model" to (Build.MODEL ?: ""),
                        // The desktop asks for the 6-char code instead of
                        // waiting on an Allow prompt.
                        "pair" to "code",
                    ),
                )
                jmdns = JmDNS.create(address).also { it.registerService(service) }
            }
        }.start()
    }

    @Synchronized
    fun stop() {
        runCatching { jmdns?.unregisterAllServices() }
        jmdns = null
    }
}

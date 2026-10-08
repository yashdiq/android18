package com.android18.service

import android.app.Application
import android.app.NotificationChannel
import android.app.NotificationManager
import dagger.hilt.android.HiltAndroidApp

/**
 * App entry point: Hilt's generated graph + the low-priority notification
 * channel the file-server foreground service posts into.
 */
@HiltAndroidApp
class Android18App : Application() {

    override fun onCreate() {
        super.onCreate()
        val manager = getSystemService(NOTIFICATION_SERVICE) as NotificationManager
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL_SERVER, "File server", NotificationManager.IMPORTANCE_LOW)
        )
        manager.createNotificationChannel(
            NotificationChannel(
                CHANNEL_PAIR_REQUESTS,
                "Pairing requests",
                NotificationManager.IMPORTANCE_HIGH,
            ).apply { description = "Allow/deny prompts when a computer asks to connect" }
        )
    }

    companion object {
        const val CHANNEL_SERVER = "server"
        const val CHANNEL_PAIR_REQUESTS = "pair_requests"
    }
}

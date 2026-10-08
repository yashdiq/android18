package com.android18.service

import android.Manifest
import android.content.pm.PackageManager
import android.content.Intent
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.android18.service.data.server.FileService
import com.android18.service.data.server.PairRequestGate
import com.android18.service.presentation.navigation.AppRoot
import com.android18.service.presentation.theme.Android18Theme
import dagger.hilt.android.AndroidEntryPoint
import javax.inject.Inject

/** Single-activity Compose host; every screen is a navigation destination. */
@AndroidEntryPoint
class MainActivity : ComponentActivity() {

    @Inject lateinit var pairGate: PairRequestGate

    /** Desktop name from a USB `CONNECT` intent awaiting the user's answer. */
    private var usbConnectRequester by mutableStateOf<String?>(null)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        requestNotificationPermission()
        captureConnectIntent(intent)
        setContent {
            Android18Theme {
                AppRoot()
                PairRequestDialog(pairGate)
                usbConnectRequester?.let { name ->
                    UsbConnectDialog(
                        name = name,
                        onAllow = {
                            usbConnectRequester = null
                            allowUsbConnect()
                        },
                        onDeny = { usbConnectRequester = null },
                    )
                }
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        captureConnectIntent(intent)
    }

    /**
     * `adb shell am start -a …CONNECT --es desktop <name>` — the desktop's
     * USB plug-in trigger. Only raises the prompt; nothing starts until
     * the user taps Allow.
     */
    private fun captureConnectIntent(intent: Intent?) {
        if (intent?.action != ACTION_CONNECT) return
        usbConnectRequester = intent.getStringExtra(EXTRA_DESKTOP)
            ?.trim()?.ifEmpty { null }?.take(MAX_NAME) ?: "a computer"
        intent.action = null // a recreate must not re-prompt
    }

    /** Allow: pre-approve the desktop's follow-up request and start the service. */
    private fun allowUsbConnect() {
        pairGate.preApprove()
        startForegroundService(Intent(this, FileService::class.java))
    }

    /**
     * API 33+ gates notifications behind a runtime grant; the pairing
     * prompt's heads-up notification needs it (the in-app dialog and the
     * 60s timeout cover a denial).
     */
    private fun requestNotificationPermission() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU &&
            checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), REQUEST_NOTIFICATIONS)
        }
    }

    companion object {
        private const val REQUEST_NOTIFICATIONS = 100
        private const val MAX_NAME = 60
        const val ACTION_CONNECT = "com.android18.service.CONNECT"
        const val EXTRA_DESKTOP = "desktop"
    }
}

/** USB plug-in prompt: Allow starts the file service for this computer. */
@Composable
private fun UsbConnectDialog(name: String, onAllow: () -> Unit, onDeny: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDeny,
        title = { Text("Allow this computer?") },
        text = { Text("\"$name\" connected over USB and wants to access your files.") },
        confirmButton = { TextButton(onClick = onAllow) { Text("Allow") } },
        dismissButton = { TextButton(onClick = onDeny) { Text("Deny") } },
    )
}

/**
 * In-app "allow this computer?" prompt mirroring the notification while
 * the app is in the foreground; answering either surface resolves the
 * same pending request.
 */
@Composable
private fun PairRequestDialog(gate: PairRequestGate) {
    val requester by gate.pendingRequester.collectAsState()
    val name = requester ?: return
    AlertDialog(
        onDismissRequest = { gate.resolve(allow = false) },
        title = { Text("Allow this computer?") },
        text = { Text("\"$name\" wants to connect to your files.") },
        confirmButton = {
            TextButton(onClick = { gate.resolve(allow = true) }) { Text("Allow") }
        },
        dismissButton = {
            TextButton(onClick = { gate.resolve(allow = false) }) { Text("Deny") }
        },
    )
}


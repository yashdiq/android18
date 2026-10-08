package com.android18.service.presentation.feature.server

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Environment
import android.provider.Settings
import android.widget.Toast
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material.icons.outlined.History
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.ContextCompat
import androidx.hilt.navigation.compose.hiltViewModel
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.android18.service.data.server.FileService
import com.android18.service.domain.model.ServerState
import com.android18.service.presentation.common.*
import com.android18.service.presentation.theme.Amber
import com.android18.service.presentation.theme.Emerald
import com.android18.service.presentation.theme.Rose
import com.android18.service.presentation.theme.Slate900
import com.android18.service.util.formatTime

/** Dashboard: server engine, pairing/security, live request journal. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ServerScreen(
    onOpenSettings: () -> Unit,
    onOpenScanner: () -> Unit,
    viewModel: ServerViewModel = hiltViewModel(),
) {
    val serverState by viewModel.serverState.collectAsStateWithLifecycle()
    val entries by viewModel.journalEntries.collectAsStateWithLifecycle()
    val clients by viewModel.clientCount.collectAsStateWithLifecycle()
    val pairing by viewModel.pairing.collectAsStateWithLifecycle()
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    // The FGS runs either way; POST_NOTIFICATIONS (API 33+) just makes the
    // persistent notification visible, so request it alongside the start.
    val notifPermission = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestPermission(),
    ) { }
    // Storage access: without All-Files Access (or a media read grant),
    // Android's FUSE layer strips file entries from listFiles() while
    // directories stay visible — the desktop then sees folders with no
    // files. Both launchers re-snapshot the grants when they return.
    var storage by remember { mutableStateOf(storageAccess(context)) }
    val allFilesLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.StartActivityForResult(),
    ) { storage = storageAccess(context) }
    val readPermission = rememberLauncherForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions(),
    ) { storage = storageAccess(context) }

    fun grantAllFiles() {
        if (Build.VERSION.SDK_INT >= 30) {
            val appPage = Intent(
                Settings.ACTION_MANAGE_APP_ALL_FILES_ACCESS_PERMISSION,
                Uri.parse("package:${context.packageName}"),
            )
            runCatching { allFilesLauncher.launch(appPage) }.onFailure {
                allFilesLauncher.launch(Intent(Settings.ACTION_MANAGE_ALL_FILES_ACCESS_PERMISSION))
            }
        } else {
            readPermission.launch(arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE))
        }
    }

    fun grantMedia() {
        val permissions = if (Build.VERSION.SDK_INT >= 33) {
            arrayOf(
                Manifest.permission.READ_MEDIA_IMAGES,
                Manifest.permission.READ_MEDIA_VIDEO,
                Manifest.permission.READ_MEDIA_AUDIO,
            )
        } else {
            arrayOf(Manifest.permission.READ_EXTERNAL_STORAGE)
        }
        readPermission.launch(permissions)
    }
    val pairCode by viewModel.pairCode.collectAsStateWithLifecycle()

    fun copy(label: String, value: String) {
        clipboard.setText(AnnotatedString(value))
        Toast.makeText(context, "$label copied", Toast.LENGTH_SHORT).show()
    }

    val running = serverState == ServerState.RUNNING

    fun setServerRunning(start: Boolean) {
        if (start) {
            if (Build.VERSION.SDK_INT >= 33 &&
                ContextCompat.checkSelfPermission(
                    context,
                    Manifest.permission.POST_NOTIFICATIONS,
                ) != PackageManager.PERMISSION_GRANTED
            ) {
                notifPermission.launch(Manifest.permission.POST_NOTIFICATIONS)
            }
            context.startForegroundService(Intent(context, FileService::class.java))
        } else {
            context.startService(
                Intent(context, FileService::class.java)
                    .setAction(FileService.ACTION_STOP),
            )
        }
        viewModel.refresh()
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .padding(16.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Text(
                "Android18",
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.weight(1f),
            )
            IconButton(onClick = onOpenSettings) {
                Icon(Icons.Outlined.Settings, contentDescription = "Settings")
            }
        }

        // Scrollable content keeps the bottom dock always in reach.
        Column(
            modifier = Modifier
                .weight(1f)
                .verticalScroll(rememberScrollState()),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
        PfsCard {
            Row(verticalAlignment = Alignment.CenterVertically) {
                StatusDot(
                    color = if (running) Emerald else MaterialTheme.colorScheme.onSurfaceVariant,
                    pulse = running,
                )
                Spacer(Modifier.width(8.dp))
                Column(Modifier.weight(1f)) {
                    Text(
                        if (running) "Running on the LAN" else "Server stopped",
                        style = MaterialTheme.typography.titleSmall,
                    )
                    Text(
                        "Foreground service · persistent notification",
                        style = MaterialTheme.typography.bodySmall,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }
            }
            if (running) {
                MonoText(
                    "http://${pairing.ip ?: "no Wi-Fi"}:${pairing.port}",
                    color = MaterialTheme.colorScheme.onSurface,
                )
                PrimaryButton(
                    text = "Scan desktop QR",
                    onClick = onOpenScanner,
                    modifier = Modifier.fillMaxWidth(),
                )
                Text(
                    "_android18._tcp.local. advertised over mDNS",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            } else {
                Text(
                    "Start the server to serve /storage/emulated/0 and advertise " +
                        "_android18._tcp.local. on the LAN.",
                    style = MaterialTheme.typography.bodyMedium,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        if (!storage.allFiles) {
            StorageAccessCard(
                access = storage,
                onGrantAllFiles = ::grantAllFiles,
                onGrantMedia = ::grantMedia,
            )
        }

        val desktop by viewModel.connectedDesktop.collectAsStateWithLifecycle()
        val connected = desktop
        if (running && connected != null) {
            PfsCard {
                SectionHeader("Desktop connected")
                MonoText(connected, color = MaterialTheme.colorScheme.onSurface)
                Text(
                    "Already paired — the pair code is hidden while a desktop is connected.",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        } else {
            PfsCard {
                SectionHeader("Pair code")
                Text(
                    pairCode.code,
                    modifier = Modifier.fillMaxWidth(),
                    style = MaterialTheme.typography.headlineLarge,
                    fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace,
                    letterSpacing = 6.sp,
                    color = if (running) {
                        MaterialTheme.colorScheme.onSurface
                    } else {
                        MaterialTheme.colorScheme.onSurfaceVariant
                    },
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                )
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    SecondaryButton(
                        text = "Copy",
                        onClick = { copy("Pair code", pairCode.code) },
                        modifier = Modifier.weight(1f),
                    )
                    SecondaryButton(
                        text = "New code",
                        onClick = {
                            viewModel.regenerateCode()
                            Toast.makeText(context, "New pair code", Toast.LENGTH_SHORT).show()
                        },
                        modifier = Modifier.weight(1f),
                    )
                }
                Text(
                    if (running) {
                        "Pick this phone in the desktop app and type the code. " +
                            "It works once and changes after use."
                    } else {
                        "Start the server, then type this code in the desktop app."
                    },
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }

        PfsCard {
            // Heartbeat /info polls are filtered from the list; the count
            // must match what the user actually sees.
            val visible = entries.filterNot { it.endpoint.startsWith("/info") }
            Row(
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.SpaceBetween,
            ) {
                SectionHeader("Requests")
                Text(
                    "${visible.size} ${if (visible.size == 1) "request" else "requests"} · " +
                        "$clients ${if (clients == 1) "client" else "clients"}",
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
            if (visible.isEmpty()) {
                EmptyState(
                    icon = Icons.Outlined.History,
                    title = "No requests yet",
                    hint = "Pair the desktop app to see live traffic here.",
                )
            } else {
                visible.takeLast(12).reversed().forEach { entry ->
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
                        MonoText(
                            "${entry.method} ${entry.endpoint}",
                            modifier = Modifier.weight(1f, fill = false),
                        )
                        MonoText(
                            "${entry.status} · ${formatTime(entry.atMillis)}",
                            color = if (entry.status >= 400) {
                                Rose
                            } else {
                                MaterialTheme.colorScheme.onSurfaceVariant
                            },
                        )
                    }
                }
            }
        }

        Spacer(Modifier.height(8.dp))
        } // scrollable content

        // Bottom-center dock: the start/stop control is always visible.
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(top = 8.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            ServerToggle(running = running, onToggle = ::setServerRunning)
            Spacer(Modifier.height(6.dp))
            Text(
                if (running) "Stop server" else "Start server",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
}

/** Storage-access snapshot backing the Server banner. */
private data class StorageAccess(val allFiles: Boolean, val media: Boolean)

private fun storageAccess(context: android.content.Context): StorageAccess = StorageAccess(
    allFiles = Build.VERSION.SDK_INT < 30 || Environment.isExternalStorageManager(),
    media = mediaReadGranted(context),
)

/** True when at least one scoped-storage read grant exposes files. */
private fun mediaReadGranted(context: android.content.Context): Boolean {
    fun granted(permission: String) =
        ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED
    return if (Build.VERSION.SDK_INT >= 33) {
        listOf(
            Manifest.permission.READ_MEDIA_IMAGES,
            Manifest.permission.READ_MEDIA_VIDEO,
            Manifest.permission.READ_MEDIA_AUDIO,
        ).any(::granted)
    } else {
        granted(Manifest.permission.READ_EXTERNAL_STORAGE)
    }
}

/**
 * Warning card shown while the app cannot read files off shared storage —
 * the state where the desktop sees folders but no files.
 */
@Composable
private fun StorageAccessCard(
    access: StorageAccess,
    onGrantAllFiles: () -> Unit,
    onGrantMedia: () -> Unit,
) {
    PfsCard {
        Row(verticalAlignment = Alignment.CenterVertically) {
            StatusDot(color = Amber)
            Spacer(Modifier.width(8.dp))
            Column(Modifier.weight(1f)) {
                Text(
                    "Storage access limited",
                    style = MaterialTheme.typography.titleSmall,
                )
                Text(
                    if (access.media) {
                        "Only media files are shared. Grant All files access to share documents and every folder."
                    } else {
                        "Android hides files until access is granted — the desktop would see folders with nothing inside."
                    },
                    style = MaterialTheme.typography.bodySmall,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                )
            }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (!access.media) {
                SecondaryButton(
                    text = if (Build.VERSION.SDK_INT >= 33) "Allow media files" else "Allow files",
                    onClick = onGrantMedia,
                    modifier = Modifier.weight(1f),
                )
            }
            PrimaryButton(
                text = "Grant all files access",
                onClick = onGrantAllFiles,
                modifier = Modifier.weight(1f),
            )
        }
    }
}

/** Big circular start/stop control; icon centered in the circle. */
@Composable
private fun ServerToggle(running: Boolean, onToggle: (Boolean) -> Unit) {
    val container = if (running) Slate900 else Emerald
    val icon = if (running) Icons.Filled.Stop else Icons.Filled.PlayArrow
    val label = if (running) "Stop server" else "Start server"
    Box(
        modifier = Modifier
            .size(64.dp)
            .clip(CircleShape)
            .background(container)
            .clickable { onToggle(!running) },
        contentAlignment = Alignment.Center,
    ) {
        Icon(
            icon,
            contentDescription = label,
            tint = Color.White,
            modifier = Modifier.size(32.dp),
        )
    }
}

package com.android18.service.presentation.feature.settings

import android.widget.Toast
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import androidx.hilt.navigation.compose.hiltViewModel
import com.android18.service.presentation.common.*
import com.android18.service.presentation.theme.Rose

/** Settings: server port, GLM key, desktop-access revocation, journal clear. */
@Composable
fun SettingsScreen(onBack: () -> Unit, viewModel: SettingsViewModel = hiltViewModel()) {
    val context = LocalContext.current
    var portText by remember { mutableStateOf(viewModel.port.toString()) }
    var portError by remember { mutableStateOf<String?>(null) }
    var keyText by remember { mutableStateOf(viewModel.glmKey) }
    var confirmRevoke by remember { mutableStateOf(false) }

    if (confirmRevoke) {
        AlertDialog(
            onDismissRequest = { confirmRevoke = false },
            title = { Text("Revoke desktop access?") },
            text = {
                Text(
                    "Paired desktops stop working immediately and must " +
                        "scan a new QR to reconnect.",
                )
            },
            confirmButton = {
                TextButton(
                    onClick = {
                        viewModel.revokeDesktopAccess()
                        confirmRevoke = false
                        Toast.makeText(context, "Desktop access revoked", Toast.LENGTH_SHORT).show()
                    },
                ) { Text("Revoke", color = Rose) }
            },
            dismissButton = {
                TextButton(onClick = { confirmRevoke = false }) { Text("Cancel") }
            },
        )
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .padding(16.dp),
        verticalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) {
                Icon(Icons.AutoMirrored.Outlined.ArrowBack, contentDescription = "Back")
            }
            Text("Settings", style = MaterialTheme.typography.titleLarge)
        }

        PfsCard {
            SectionHeader("Server")
            OutlinedTextField(
                value = portText,
                onValueChange = { portText = it },
                label = { Text("LAN port") },
                isError = portError != null,
                supportingText = portError?.let { error ->
                    { Text(error, color = MaterialTheme.colorScheme.error) }
                },
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            PrimaryButton(
                text = "Save port (restart server to apply)",
                onClick = {
                    portError = viewModel.savePort(portText)
                    if (portError == null) {
                        Toast.makeText(context, "Port saved", Toast.LENGTH_SHORT).show()
                    }
                },
                modifier = Modifier.fillMaxWidth(),
            )
            SecondaryButton(
                text = "Clear request journal",
                onClick = {
                    viewModel.clearJournal()
                    Toast.makeText(context, "Journal cleared", Toast.LENGTH_SHORT).show()
                },
                modifier = Modifier.fillMaxWidth(),
            )
        }

        PfsCard {
            SectionHeader("AI search")
            OutlinedTextField(
                value = keyText,
                onValueChange = { keyText = it },
                label = { Text("GLM (Z.AI) API key") },
                singleLine = true,
                modifier = Modifier.fillMaxWidth(),
            )
            PrimaryButton(
                text = "Save key",
                onClick = {
                    viewModel.saveGlmKey(keyText.trim())
                    Toast.makeText(context, "Key saved", Toast.LENGTH_SHORT).show()
                },
                modifier = Modifier.fillMaxWidth(),
            )
            Text(
                "Leave empty to use the offline heuristic engine. Requests go " +
                    "directly from this phone to Google's API.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        PfsCard {
            SectionHeader("Security")
            DangerButton(
                text = "Revoke desktop access",
                onClick = { confirmRevoke = true },
                modifier = Modifier.fillMaxWidth(),
            )
            Text(
                "Regenerates the pairing token. Paired desktops get 401s " +
                    "and must scan a new QR to reconnect.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }

        PfsCard {
            SectionHeader("About")
            MetaRow("App", "Android18 mobile")
            MetaRow("Version", "1.0.0")
            MetaRow("Desktop counterpart", "android-18 (macOS, GPUI)")
            Text(
                "Files stay on your devices; the LAN transport is token-guarded " +
                    "and never leaves the local network.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        Spacer(Modifier.height(16.dp))
    }
}

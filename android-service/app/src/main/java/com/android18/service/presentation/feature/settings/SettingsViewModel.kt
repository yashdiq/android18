package com.android18.service.presentation.feature.settings

import androidx.lifecycle.ViewModel
import com.android18.service.data.local.PrefsDataSource
import com.android18.service.data.repository.PairingRepository
import com.android18.service.data.server.ServerJournal
import dagger.hilt.android.lifecycle.HiltViewModel
import javax.inject.Inject

/** Settings persistence: port, GLM key, token reset, journal clear. */
@HiltViewModel
class SettingsViewModel @Inject constructor(
    private val prefs: PrefsDataSource,
    private val pairing: PairingRepository,
    private val journal: ServerJournal,
) : ViewModel() {

    val port: Int get() = prefs.port

    val glmKey: String get() = prefs.glmKey

    /** Validates and persists the port; returns an error message or null. */
    fun savePort(value: String): String? {
        val parsed = value.trim().toIntOrNull() ?: return "Port must be a number"
        if (parsed !in 1024..65535) return "Port must be 1024–65535"
        prefs.port = parsed
        return null
    }

    fun saveGlmKey(value: String) {
        prefs.glmKey = value
    }

    /** Rotates the pairing token — paired desktops get 401s until they
     *  re-pair. The token itself never leaves the app. */
    fun revokeDesktopAccess() {
        pairing.regenerateToken()
    }

    fun clearJournal() = journal.clear()
}

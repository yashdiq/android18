package com.android18.service.presentation.feature.server

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.android18.service.data.repository.PairingRepository
import com.android18.service.data.server.PairCodeVault
import com.android18.service.data.server.ServerJournal
import com.android18.service.domain.model.JournalEntry
import com.android18.service.domain.model.PairingInfo
import com.android18.service.domain.model.ServerState
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.flow.flow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import com.android18.service.util.activeDesktop
import javax.inject.Inject

/** Dashboard state: server lifecycle, pairing identity, live request journal. */
@HiltViewModel
class ServerViewModel @Inject constructor(
    private val pairingRepo: PairingRepository,
    private val codes: PairCodeVault,
    journal: ServerJournal,
) : ViewModel() {

    /** The 6-char code on screen; rotates after use, lockout or expiry. */
    val pairCode: StateFlow<PairCodeVault.Snapshot> = codes.snapshot

    init {
        // Expiry rotates lazily; poll so the screen never shows a code
        // the service has already retired.
        viewModelScope.launch {
            while (true) {
                codes.current()
                delay(15_000)
            }
        }
    }

    fun regenerateCode() = codes.regenerate()

    /** Address of the desktop currently connected, or null (see [activeDesktop]). */
    val connectedDesktop: StateFlow<String?> = combine(
        journal.entries,
        flow { while (true) { emit(Unit); delay(5_000) } },
    ) { entries, _ -> activeDesktop(entries, System.currentTimeMillis()) }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), null)

    val serverState: StateFlow<ServerState> = journal.state
    val journalEntries: StateFlow<List<JournalEntry>> = journal.entries

    /** Distinct client IPs seen in the journal window. */
    val clientCount: StateFlow<Int> = journal.entries
        .map { entries -> entries.mapNotNull { it.client }.distinct().size }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), 0)

    private val _pairing = MutableStateFlow(pairingRepo.info())
    val pairing: StateFlow<PairingInfo> = _pairing.asStateFlow()

    fun refresh() {
        _pairing.value = pairingRepo.info()
    }

}

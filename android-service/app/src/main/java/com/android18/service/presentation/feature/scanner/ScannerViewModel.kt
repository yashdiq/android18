package com.android18.service.presentation.feature.scanner

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import com.android18.service.data.remote.DesktopQrPayload
import com.android18.service.data.remote.DesktopPairClient
import com.android18.service.data.remote.PairOutcome
import com.android18.service.data.repository.PairingRepository
import com.android18.service.data.server.ServerJournal
import com.android18.service.domain.model.ServerState
import dagger.hilt.android.lifecycle.HiltViewModel
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharingStarted
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.map
import kotlinx.coroutines.flow.stateIn
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import javax.inject.Inject

private const val SERVER_WAIT_TRIES = 32
private const val SERVER_WAIT_STEP_MS = 250L
private const val FAIL_AUTO_RESET_MS = 3_000L

/** Scanner UI state machine. */
sealed interface ScanState {
    /** Camera live, waiting for a code. */
    data object Idle : ScanState

    /** QR parsed; waiting for the user to Allow or Deny this computer. */
    data class Confirm(val desktop: DesktopQrPayload, val endpoint: String) : ScanState

    /** Allowed; waiting for the file server to start listening. */
    data class Starting(val endpoint: String) : ScanState

    /** Allowed; posting to the desktop listener. */
    data class Posting(val endpoint: String) : ScanState

    /** Desktop answered 200 — the caller starts the service and pops. */
    data object Success : ScanState

    /** HTTP rejection or network error; auto-falls-back to [Idle]. */
    data class Failed(val message: String) : ScanState
}

/**
 * Drives QR pairing from the phone side: parse the desktop's QR, ask the
 * user to allow that computer, then POST this phone's identity to the
 * desktop's one-shot listener and surface the outcome. Service start
 * stays in the screen (it owns the context).
 */
@HiltViewModel
class ScannerViewModel @Inject constructor(
    private val pairingRepo: PairingRepository,
    private val client: DesktopPairClient,
    journal: ServerJournal,
) : ViewModel() {

    /** Whether the file server is already up (pairing success may need it). */
    val serverRunning: StateFlow<Boolean> = journal.state
        .map { it == ServerState.RUNNING }
        .stateIn(viewModelScope, SharingStarted.WhileSubscribed(5_000), false)

    private val _state = MutableStateFlow<ScanState>(ScanState.Idle)
    val state: StateFlow<ScanState> = _state.asStateFlow()

    /** Transient scan hint (e.g. "not a pairing code"). Unlike [ScanState]
     *  it never pauses the analyzer — the next decoded frame clears it, so
     *  a stray code can't look like a bricked scanner. */
    private val _hint = MutableStateFlow<String?>(null)
    val hint: StateFlow<String?> = _hint.asStateFlow()

    /** A decoded QR text arrived (called once per frame batch; the VM
     *  de-duplicates and ignores codes while a pairing is in flight). */
    fun onQrScanned(raw: String) {
        if (_state.value is ScanState.Posting || _state.value is ScanState.Confirm) return
        val desktop: DesktopQrPayload = client.parseQrPayload(raw)
            ?: run {
                // Not one of ours — keep scanning. A sticky Failed here
                // used to pause the analyzer until a manual tap, which
                // read as a dead scanner whenever the viewfinder caught
                // a stray code.
                _hint.value = "Not an Android18 pairing code"
                return
            }
        _hint.value = null
        val host = desktop.ip.ifBlank { desktop.endpoint.substringBeforeLast(':') }
        val port = desktop.port.takeIf { it in 1..65535 }
            ?: desktop.endpoint.substringAfterLast(':').toIntOrNull()
            ?: 0
        _state.value = ScanState.Confirm(desktop, "$host:$port")
    }

    /** The user tapped Allow on the confirm dialog: pair with the desktop. */
    fun allow() {
        val confirm = _state.value as? ScanState.Confirm ?: return
        _state.value = ScanState.Starting(confirm.endpoint)
        viewModelScope.launch {
            // The desktop dials back the moment it gets our POST, so the
            // server must already be bound — the service starts async.
            if (!waitForServer(pairingRepo.info().port)) {
                fail("The file server did not start — try again")
                return@launch
            }
            _state.value = ScanState.Posting(confirm.endpoint)
            when (val outcome = client.pair(confirm.desktop, pairingRepo.qrDto())) {
                PairOutcome.Paired -> _state.value = ScanState.Success
                is PairOutcome.Rejected -> fail(
                    if (outcome.status == 403) {
                        "Desktop rejected the code — it may be expired. Ask for a new QR."
                    } else {
                        "Desktop rejected the pairing (HTTP ${outcome.status})"
                    },
                )
                is PairOutcome.Error -> fail("Could not reach the desktop: ${outcome.message}")
            }
        }
    }

    /** Shows the failure, then falls back to scanning on its own — a
     *  sticky error left the analyzer paused until tapped. */
    private fun fail(message: String) {
        _state.value = ScanState.Failed(message)
        viewModelScope.launch {
            delay(FAIL_AUTO_RESET_MS)
            if (_state.value is ScanState.Failed) _state.value = ScanState.Idle
        }
    }

    /** Polls `127.0.0.1:port` until something accepts (≤ 8 s). */
    private suspend fun waitForServer(port: Int): Boolean = withContext(Dispatchers.IO) {
        repeat(SERVER_WAIT_TRIES) {
            val open = runCatching {
                java.net.Socket().use { it.connect(java.net.InetSocketAddress("127.0.0.1", port), 200) }
            }.isSuccess
            if (open) return@withContext true
            delay(SERVER_WAIT_STEP_MS)
        }
        false
    }

    /** Deny: nothing is sent and nothing starts; back to scanning. */
    fun deny() {
        if (_state.value is ScanState.Confirm) _state.value = ScanState.Idle
    }

    /** Back to scanning after a failure. */
    fun reset() {
        _hint.value = null
        _state.value = ScanState.Idle
    }
}

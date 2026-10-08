package com.android18.service.data.server

import com.android18.service.domain.model.JournalEntry
import com.android18.service.domain.model.ServerState
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import javax.inject.Inject
import javax.inject.Singleton

/**
 * Shared server state: the running flag plus the last [MAX_ENTRIES] requests,
 * mirroring the desktop client's 80-entry rolling request log. The service
 * writes; the dashboard screen observes.
 */
@Singleton
class ServerJournal @Inject constructor() {

    private val _state = MutableStateFlow(ServerState.STOPPED)
    val state: StateFlow<ServerState> = _state.asStateFlow()

    private val _entries = MutableStateFlow<List<JournalEntry>>(emptyList())
    val entries: StateFlow<List<JournalEntry>> = _entries.asStateFlow()

    private val ring = ArrayDeque<JournalEntry>()

    fun markRunning(running: Boolean) {
        _state.value = if (running) ServerState.RUNNING else ServerState.STOPPED
    }

    fun log(method: String, endpoint: String, status: Int, client: String?) {
        val entry = JournalEntry(method, endpoint, status, client, System.currentTimeMillis())
        synchronized(ring) {
            ring.addLast(entry)
            while (ring.size > MAX_ENTRIES) ring.removeFirst()
            _entries.value = ring.toList()
        }
    }

    fun clear() {
        synchronized(ring) {
            ring.clear()
            _entries.value = emptyList()
        }
    }

    companion object {
        const val MAX_ENTRIES = 80
    }
}

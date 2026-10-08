package com.android18.service

import com.android18.service.data.server.ServerJournal
import com.android18.service.domain.model.ServerState
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class ServerJournalTest {

    @Test
    fun ringCapsAtEightyKeepingNewest() {
        val journal = ServerJournal()
        repeat(90) { i -> journal.log("GET", "/list?path=/x$i", 200, "192.168.1.50") }
        assertEquals(ServerJournal.MAX_ENTRIES, journal.entries.value.size)
        assertEquals("/list?path=/x89", journal.entries.value.last().endpoint)
        assertTrue(journal.entries.value.none { it.endpoint.endsWith("/x0") })
    }

    @Test
    fun distinctClientsAreCountableFromEntries() {
        val journal = ServerJournal()
        journal.log("GET", "/list", 200, "192.168.1.10")
        journal.log("GET", "/search", 200, "192.168.1.11")
        journal.log("GET", "/stats", 200, "192.168.1.10")
        val clients = journal.entries.value.mapNotNull { it.client }.distinct()
        assertEquals(2, clients.size)
    }

    @Test
    fun stateTogglesAndClearEmpties() {
        val journal = ServerJournal()
        assertEquals(ServerState.STOPPED, journal.state.value)
        journal.markRunning(true)
        assertEquals(ServerState.RUNNING, journal.state.value)
        journal.log("GET", "/list", 200, null)
        journal.clear()
        assertTrue(journal.entries.value.isEmpty())
        journal.markRunning(false)
        assertEquals(ServerState.STOPPED, journal.state.value)
    }
}

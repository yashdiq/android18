package com.android18.service

import com.android18.service.domain.model.FileEntry
import com.android18.service.domain.usecase.SearchFilesUseCase
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Pure offline ranking used whenever GLM is unavailable or unconfigured. */
class HeuristicSearchTest {

    private fun file(
        name: String,
        size: Long = 100,
        modifiedAt: Long = 0L,
        path: String = "/storage/emulated/0/$name",
    ) = FileEntry(name = name, path = path, isDir = false, size = size, modifiedAt = modifiedAt)

    @Test
    fun ranksExactNameMatchFirst() {
        val outcome = SearchFilesUseCase.heuristic(
            "screenshot",
            listOf(file("screenshot.png"), file("notes.txt"), file("screenshots-old.jpg")),
        )
        assertEquals(SearchFilesUseCase.ENGINE_HEURISTIC, outcome.engine)
        assertEquals("screenshot.png", outcome.matches.first().name)
        assertTrue(outcome.matches.none { it.name == "notes.txt" })
    }

    @Test
    fun directoriesAreNeverReturned() {
        val dir = FileEntry(
            name = "screenshots",
            path = "/storage/emulated/0/screenshots",
            isDir = true,
            size = 0,
            modifiedAt = 0,
        )
        val outcome = SearchFilesUseCase.heuristic("screenshot", listOf(dir))
        assertTrue(outcome.matches.isEmpty())
        assertTrue(outcome.summary.startsWith("No matches"))
    }

    @Test
    fun largeBoostPrefersBigFiles() {
        val big = file("trip-video.mp4", size = 50_000_000)
        val small = file("trip-video-preview.mp4", size = 5_000)
        val outcome = SearchFilesUseCase.heuristic("large trip video", listOf(small, big))
        assertEquals(big.name, outcome.matches.first().name)
    }

    @Test
    fun recentBoostRanksNewerFilesFirst() {
        val now = System.currentTimeMillis()
        val fresh = file("receipt.pdf", modifiedAt = now)
        val stale = file("receipt-2019.pdf", modifiedAt = now - 365L * 24 * 3600 * 1000)
        val outcome = SearchFilesUseCase.heuristic("recent receipt", listOf(stale, fresh))
        assertEquals(fresh.name, outcome.matches.first().name)
    }

    @Test
    fun confidenceBandsFollowScore() {
        // Contains-only name match scores 4 → low.
        val low = SearchFilesUseCase.heuristic("invoice", listOf(file("invoice.pdf")))
        assertEquals("low", low.matches.single().confidence)
        // Exact name match scores 6 → medium.
        val medium = SearchFilesUseCase.heuristic("invoice", listOf(file("invoice")))
        assertEquals("medium", medium.matches.single().confidence)
        // Exact name + "large" size boost scores 9 → high.
        val high = SearchFilesUseCase.heuristic(
            "large report",
            listOf(file("report", size = 50_000_000)),
        )
        assertEquals("high", high.matches.single().confidence)
    }
}

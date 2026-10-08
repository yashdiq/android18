package com.android18.service.domain.usecase

import com.android18.service.data.local.FileAccessSource
import com.android18.service.data.local.PrefsDataSource
import com.android18.service.data.remote.GlmClient
import com.android18.service.domain.model.FileEntry
import com.android18.service.domain.model.SearchHit
import com.android18.service.domain.model.SearchOutcome
import javax.inject.Inject

/**
 * Natural-language file search: GLM (Z.AI) when an API key is
 * configured, offline name/path/size/recency heuristics otherwise — and on
 * any GLM failure.
 */
class SearchFilesUseCase @Inject constructor(
    private val files: FileAccessSource,
    private val prefs: PrefsDataSource,
    private val glm: GlmClient,
) {

    suspend operator fun invoke(query: String, index: List<FileEntry>? = null): SearchOutcome {
        val entries = index ?: files.index()
        val key = prefs.glmKey
        if (key.length < MIN_KEY_LENGTH) {
            return heuristic(query, entries).copy(warning = "No GLM API key set on the phone (Settings)")
        }
        val result = glm.search(query, entries, key)
        result.getOrNull()?.let { return it }
        val reason = result.exceptionOrNull()?.message ?: "request failed"
        return heuristic(query, entries).copy(warning = reason)
    }

    companion object {
        const val MIN_KEY_LENGTH = 10
        const val ENGINE_HEURISTIC = "heuristic"

        /** Pure offline ranking — unit tested. */
        fun heuristic(query: String, entries: List<FileEntry>): SearchOutcome {
            val terms = query.lowercase().split(Regex("[^a-z0-9]+")).filter { it.length > 1 }
            val weekAgo = System.currentTimeMillis() - 7L * 24 * 3600 * 1000
            val hits = entries.mapNotNull { entry ->
                var score = 0
                val haystack = entry.name.lowercase()
                for (term in terms) {
                    when {
                        haystack == term -> score += 6
                        haystack.contains(term) -> score += 4
                        entry.path.lowercase().contains(term) -> score += 2
                    }
                }
                if (query.contains("large", ignoreCase = true) && entry.size > 20_000_000) score += 3
                if (query.contains("recent", ignoreCase = true) && entry.modifiedAt > weekAgo) score += 3
                if (score == 0 || entry.isDir) return@mapNotNull null
                SearchHit(
                    path = entry.path,
                    name = entry.name,
                    isDir = false,
                    size = entry.size,
                    reason = "name/size/recency heuristics",
                    confidence = when {
                        score >= 8 -> "high"
                        score >= 5 -> "medium"
                        else -> "low"
                    },
                ) to score
            }.sortedByDescending { it.second }.take(12).map { it.first }
            return SearchOutcome(
                summary = if (hits.isEmpty()) {
                    "No matches for “$query”"
                } else {
                    "Top matches for “$query” (offline heuristics)"
                },
                matches = hits,
                engine = ENGINE_HEURISTIC,
            )
        }
    }
}

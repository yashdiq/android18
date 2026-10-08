package com.android18.service.domain.model

/** One row of a folder listing (domain shape — wire DTOs live in data). */
data class FileEntry(
    val name: String,
    val path: String,
    val isDir: Boolean,
    val size: Long,
    val modifiedAt: Long,
    val extension: String = "",
    val itemCount: Long? = null,
    val mimeType: String? = null,
)

/** Storage card snapshot. */
data class StorageStats(val totalBytes: Long, val availableBytes: Long) {
    val usedBytes: Long get() = (totalBytes - availableBytes).coerceIn(0, totalBytes)
    val usedFraction: Float get() = if (totalBytes <= 0) 0f else usedBytes.toFloat() / totalBytes
}

/** One natural-language search result. */
data class SearchHit(
    val path: String,
    val name: String,
    val isDir: Boolean,
    val size: Long,
    val reason: String,
    val confidence: String,
)

/** Full search reply: summary line + ranked hits + which engine produced it. */
data class SearchOutcome(
    val summary: String,
    val matches: List<SearchHit>,
    val engine: String,
    /** Why the AI engine was skipped, when a fallback answered. */
    val warning: String? = null,
)

/** Everything the pairing sheet / desktop Devices modal needs. */
data class PairingInfo(
    val ip: String?,
    val port: Int,
    val token: String,
    val deviceId: String,
)

/** One row of the rolling request journal shown on the dashboard. */
data class JournalEntry(
    val method: String,
    val endpoint: String,
    val status: Int,
    val client: String?,
    val atMillis: Long,
)

/** Lifecycle of the embedded HTTP server. */
enum class ServerState { STOPPED, RUNNING }

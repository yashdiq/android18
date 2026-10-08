package com.android18.service.data.remote

import com.android18.service.domain.model.FileEntry
import com.android18.service.domain.model.SearchHit
import com.android18.service.domain.model.SearchOutcome
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import java.net.HttpURLConnection
import java.net.URL
import javax.inject.Inject

/** GLM (Z.AI, OpenAI-compatible) natural-language search over a flat file index. */
class GlmClient @Inject constructor() {

    /** Ranked results, or a failure carrying a short human-readable reason. */
    suspend fun search(
        query: String,
        index: List<FileEntry>,
        apiKey: String,
        model: String = DEFAULT_MODEL,
    ): Result<SearchOutcome> = withContext(Dispatchers.IO) {
        runCatching { callGlm(query, index, apiKey, model) }
    }

    private fun callGlm(query: String, all: List<FileEntry>, key: String, model: String): SearchOutcome {
        // Keep the prompt inside the model context: files only, newest first.
        val entries = all.asSequence().filter { !it.isDir }
            .sortedByDescending { it.modifiedAt }.take(MAX_INDEX).toList()
        val catalog = entries.joinToString("\n") { entry ->
            listOf(entry.path, "${entry.size}b", entry.extension).joinToString("|")
        }
        val prompt = """
            You are the search engine of an Android file manager. Given this file index
            (path|size|extension) and a natural-language query, return STRICT JSON only:
            {"summary": string, "matches": [{"path": string, "reason": string, "confidence": "high"|"medium"|"low"}]}.
            Pick at most 12 matches; paths must come from the index verbatim.

            Query: $query

            Index:
            $catalog
        """.trimIndent()

        val connection = URL(ENDPOINT).openConnection() as HttpURLConnection
        connection.requestMethod = "POST"
        connection.doOutput = true
        connection.connectTimeout = 10_000
        connection.readTimeout = 60_000
        connection.setRequestProperty("Content-Type", "application/json")
        connection.setRequestProperty("Authorization", "Bearer $key")
        val payload = buildJsonObject {
            put("model", model)
            put("temperature", 0.1)
            put("stream", false)
            put("thinking", buildJsonObject { put("type", "disabled") })
            put("response_format", buildJsonObject { put("type", "json_object") })
            put("messages", buildJsonArray {
                add(buildJsonObject {
                    put("role", "user")
                    put("content", prompt)
                })
            })
        }.toString()
        connection.outputStream.use { stream -> stream.write(payload.toByteArray()) }
        if (connection.responseCode !in 200..299) {
            val detail = connection.errorStream?.bufferedReader()?.use { it.readText() }.orEmpty().take(200)
            error("GLM HTTP ${connection.responseCode} $detail".trim())
        }
        val body = connection.inputStream.bufferedReader().use { it.readText() }
        val text = Json.parseToJsonElement(body)
            .jsonObject["choices"]!!
            .jsonArray[0]
            .jsonObject["message"]!!
            .jsonObject["content"]!!
            .jsonPrimitive.content
        val parsed = Json.parseToJsonElement(extractJson(text)).jsonObject
        val byPath = entries.associateBy { it.path }
        val matches = parsed["matches"]?.jsonArray.orEmpty().mapNotNull { element ->
            val match = element.jsonObject
            val path = match["path"]?.jsonPrimitive?.contentOrNull ?: return@mapNotNull null
            val entry = byPath[path] ?: return@mapNotNull null
            SearchHit(
                path = path,
                name = entry.name,
                isDir = entry.isDir,
                size = entry.size,
                reason = match["reason"]?.jsonPrimitive?.contentOrNull ?: model,
                confidence = match["confidence"]?.jsonPrimitive?.contentOrNull?.lowercase() ?: "medium",
            )
        }
        return SearchOutcome(
            summary = parsed["summary"]?.jsonPrimitive?.contentOrNull ?: "GLM results for “$query”",
            matches = matches,
            engine = model,
        )
    }

    /** Strips ```json fences / chatter: first `{` through last `}`. */
    private fun extractJson(text: String): String {
        val start = text.indexOf('{')
        val end = text.lastIndexOf('}')
        require(start >= 0 && end > start) { "GLM reply was not JSON" }
        return text.substring(start, end + 1)
    }

    companion object {
        const val ENDPOINT = "https://api.z.ai/api/paas/v4/chat/completions"
        const val DEFAULT_MODEL = "glm-4.5-flash"
        private const val MAX_INDEX = 1500
    }
}

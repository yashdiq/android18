package com.android18.service.data.local

import android.content.Context
import com.android18.service.util.newPairingToken
import dagger.hilt.android.qualifiers.ApplicationContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.encodeToString
import kotlinx.serialization.json.Json
import javax.inject.Inject
import javax.inject.Singleton

/** Persisted color tag on a path (phone-side only; surfaced in listings). */
@Serializable
data class TagEntry(val path: String, val color: String)

/**
 * SharedPreferences-backed settings: pairing token, stable device id,
 * server port, GLM key, and the phone-local color tags.
 */
@Singleton
class PrefsDataSource @Inject constructor(@ApplicationContext context: Context) {

    private val prefs = context.getSharedPreferences("android18", Context.MODE_PRIVATE)
    private val json = Json { ignoreUnknownKeys = true }

    var token: String
        get() = prefs.getString(KEY_TOKEN, null)
            ?: newPairingToken().also { fresh -> prefs.edit().putString(KEY_TOKEN, fresh).apply() }
        private set(value) {
            prefs.edit().putString(KEY_TOKEN, value).apply()
        }

    fun regenerateToken(): String = newPairingToken().also { fresh -> token = fresh }

    val deviceId: String
        get() {
            prefs.getString(KEY_DEVICE_ID, null)?.let { return it }
            val id = "android-" + newPairingToken()
            prefs.edit().putString(KEY_DEVICE_ID, id).apply()
            return id
        }

    var port: Int
        get() = prefs.getInt(KEY_PORT, DEFAULT_PORT)
        set(value) = prefs.edit().putInt(KEY_PORT, value).apply()

    var glmKey: String
        get() = prefs.getString(KEY_GLM, "").orEmpty()
        set(value) = prefs.edit().putString(KEY_GLM, value.trim()).apply()

    var tags: List<TagEntry>
        get() = prefs.getString(KEY_TAGS, null)
            ?.let { saved -> runCatching { json.decodeFromString<List<TagEntry>>(saved) }.getOrNull() }
            .orEmpty()
        private set(value) = prefs.edit().putString(KEY_TAGS, json.encodeToString(value)).apply()

    fun tagFor(path: String): String? = tags.firstOrNull { it.path == path }?.color

    fun setTag(path: String, color: String?) {
        tags = tags.filterNot { it.path == path } + listOfNotNull(color?.let { TagEntry(path, it) })
    }

    companion object {
        const val DEFAULT_PORT = 8080
        private const val KEY_TOKEN = "pairing_token"
        private const val KEY_DEVICE_ID = "device_id"
        private const val KEY_PORT = "server_port"
        private const val KEY_GLM = "glm_api_key"
        private const val KEY_TAGS = "color_tags"
    }
}

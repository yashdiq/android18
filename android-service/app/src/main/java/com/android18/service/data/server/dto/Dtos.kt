package com.android18.service.data.server.dto

import kotlinx.serialization.Serializable

/** Wire DTOs mirroring `android18-core`'s serde contract (snake_case). */

@Serializable
data class EntryDto(
    val extension: String = "",
    val name: String,
    val path: String,
    val dir: Boolean,
    val size: Long = 0,
    val mtime: Long = 0,
    val mime_type: String? = null,
    val item_count: Long? = null,
    val is_pinned: Boolean = false,
    val color_tag: String? = null,
    val content: String? = null,
)

@Serializable
data class DeviceDto(
    val id: String,
    val name: String,
    val model: String,
    val transport: String = "wifi",
    val base_url: String = "",
    val status: String = "connected",
    val port: Int = 8080,
    val storage_used_bytes: Long = 0,
    val storage_total_bytes: Long = 0,
    val battery_percent: Int? = null,
    val android_version: String = "",
    val ip_address: String? = null,
)

@Serializable
data class SearchMatchDto(
    val path: String,
    val reason: String,
    val confidence: String,
)

@Serializable
data class SearchResultDto(
    val summary: String,
    val matches: List<SearchMatchDto>,
    val engine: String? = null,
    val warning: String? = null,
)

@Serializable
data class SearchRequestDto(
    val query: String,
)

/** QR payload for desktop pairing (compact JSON so any scanner can read it). */
@Serializable
data class PairingQrDto(
    val name: String,
    val ip: String,
    val port: Int,
    val token: String,
    val deviceId: String,
)

/**
 * Body of a desktop's tokenless `POST /pair-request`. A non-empty [code]
 * is the phone's 6-char pair code (granted without a prompt); without
 * one the user is asked to allow the computer.
 */
@Serializable
data class PairRequestDto(val name: String = "", val code: String = "")

/** 200 answer to an allowed `/pair-request`: identity + pairing token. */
@Serializable
data class PairGrantDto(
    val name: String,
    val deviceId: String,
    val token: String,
)

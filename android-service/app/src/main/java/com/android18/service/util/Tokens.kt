package com.android18.service.util

import java.util.UUID

/**
 * 12-hex-char pairing token — comfortably over the desktop client's
 * "at least 8 characters" rule and easy to read in chunks.
 */
fun newPairingToken(): String = UUID.randomUUID().toString().replace("-", "").take(12)

/** Crockford base32: digits and letters minus the look-alikes `I L O U`. */
private const val PAIR_CODE_ALPHABET = "0123456789ABCDEFGHJKMNPQRSTVWXYZ"
const val PAIR_CODE_LENGTH = 6

private val secureRandom = java.security.SecureRandom()

/** A fresh 6-char pair code (~30 bits) from [java.security.SecureRandom], e.g. `0X1D8C`. */
fun newPairCode(): String = buildString(PAIR_CODE_LENGTH) {
    repeat(PAIR_CODE_LENGTH) { append(PAIR_CODE_ALPHABET[secureRandom.nextInt(PAIR_CODE_ALPHABET.length)]) }
}

/**
 * Canonical form of a typed code: uppercase, separators dropped, and the
 * look-alikes folded (`O`→`0`, `I`/`L`→`1`). Null unless the result is a
 * valid [PAIR_CODE_LENGTH]-char code.
 */
fun normalizePairCode(input: String): String? {
    val code = input.filter { it.isLetterOrDigit() }.uppercase().map {
        when (it) {
            'O' -> '0'
            'I', 'L' -> '1'
            else -> it
        }
    }.joinToString("")
    return code.takeIf { it.length == PAIR_CODE_LENGTH && it.all { c -> c in PAIR_CODE_ALPHABET } }
}

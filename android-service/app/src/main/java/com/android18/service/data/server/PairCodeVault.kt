package com.android18.service.data.server

import com.android18.service.util.newPairCode
import com.android18.service.util.normalizePairCode
import java.security.MessageDigest
import javax.inject.Inject
import javax.inject.Singleton
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow

/**
 * The phone's 6-char pair code. A desktop that types it into
 * `POST /pair-request` is granted without an Allow prompt — the user
 * reading the code off this screen is the consent.
 *
 * The code is single-use and short-lived: it rotates after a success,
 * after [MAX_FAILURES] wrong guesses (which also locks verification for
 * [LOCKOUT_MS]) and once [TTL_MS] has passed. Held in memory only.
 */
@Singleton
class PairCodeVault internal constructor(
    private val now: () -> Long,
    private val generate: () -> String,
) {
    @Inject constructor() : this(System::currentTimeMillis, ::newPairCode)

    /** The code on screen and when it stops being valid (epoch millis). */
    data class Snapshot(val code: String, val expiresAt: Long)

    sealed interface Verdict {
        data object Ok : Verdict
        data object Wrong : Verdict
        data class Locked(val retryAfterMs: Long) : Verdict
    }

    private val lock = Any()
    private var failures = 0
    private var lockedUntil = 0L
    private val _snapshot = MutableStateFlow(fresh())
    val snapshot: StateFlow<Snapshot> = _snapshot.asStateFlow()

    /** The current code, rotating first when it has expired. */
    fun current(): String = synchronized(lock) { rotateIfExpired().code }

    fun regenerate() = synchronized(lock) { rotate() }

    fun verify(candidate: String): Verdict = synchronized(lock) {
        val time = now()
        if (time < lockedUntil) return Verdict.Locked(lockedUntil - time)
        val typed = normalizePairCode(candidate)
        val actual = rotateIfExpired().code
        val matches = typed != null &&
            MessageDigest.isEqual(typed.toByteArray(), actual.toByteArray())
        when {
            matches -> {
                rotate()
                Verdict.Ok
            }
            ++failures >= MAX_FAILURES -> {
                lockedUntil = time + LOCKOUT_MS
                rotate()
                Verdict.Wrong
            }
            else -> Verdict.Wrong
        }
    }

    private fun fresh() = Snapshot(generate(), now() + TTL_MS)

    private fun rotate(): Snapshot {
        failures = 0
        return fresh().also { _snapshot.value = it }
    }

    private fun rotateIfExpired(): Snapshot {
        val snap = _snapshot.value
        return if (now() >= snap.expiresAt) rotate() else snap
    }

    companion object {
        const val MAX_FAILURES = 5
        const val LOCKOUT_MS = 30_000L
        const val TTL_MS = 10 * 60_000L
    }
}

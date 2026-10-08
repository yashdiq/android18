package com.android18.service

import com.android18.service.data.server.PairCodeVault
import com.android18.service.data.server.PairCodeVault.Verdict
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PairCodeVaultTest {

    private var clock = 1_000L
    private var counter = 0

    private fun vault() = PairCodeVault(
        now = { clock },
        generate = { "ABCDE" + (counter++ % 10) },
    )

    @Test
    fun rightCodeGrantsOnceThenRotates() {
        val vault = vault()
        val first = vault.current()
        assertEquals(Verdict.Ok, vault.verify(first.lowercase()))
        assertNotEquals(first, vault.current())
        assertEquals(Verdict.Wrong, vault.verify(first))
    }

    @Test
    fun fiveWrongGuessesRotateAndLockThenRecover() {
        val vault = vault()
        val original = vault.current()
        repeat(PairCodeVault.MAX_FAILURES) { assertEquals(Verdict.Wrong, vault.verify("ZZZZZZ")) }
        assertNotEquals(original, vault.current())
        val locked = vault.verify(vault.current())
        assertTrue(locked is Verdict.Locked)
        clock += PairCodeVault.LOCKOUT_MS + 1
        assertEquals(Verdict.Ok, vault.verify(vault.current()))
    }

    @Test
    fun codeExpiresAfterTtl() {
        val vault = vault()
        val original = vault.current()
        clock += PairCodeVault.TTL_MS + 1
        assertNotEquals(original, vault.current())
    }

    @Test
    fun regenerateReplacesTheCode() {
        val vault = vault()
        val original = vault.current()
        vault.regenerate()
        assertNotEquals(original, vault.current())
    }
}

package com.android18.service

import com.android18.service.util.newPairingToken
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class TokensTest {

    @Test
    fun tokenIsTwelveLowercaseHexChars() {
        repeat(50) {
            val token = newPairingToken()
            assertEquals(12, token.length)
            assertTrue(token.all { it in "0123456789abcdef" })
        }
    }

    @Test
    fun tokensDoNotRepeat() {
        assertNotEquals(newPairingToken(), newPairingToken())
    }
}

class PairCodeTest {

    @Test
    fun codeIsSixCrockfordChars() {
        repeat(100) {
            val code = com.android18.service.util.newPairCode()
            assertEquals(6, code.length)
            assertTrue(code.all { it in "0123456789ABCDEFGHJKMNPQRSTVWXYZ" })
        }
    }

    @Test
    fun normalizeFoldsLookalikesAndRejectsBadInput() {
        val n = { input: String -> com.android18.service.util.normalizePairCode(input) }
        assertEquals("0X1D8C", n("0x1d8c"))
        assertEquals("0X1D8C", n("Ox1-d8c"))
        assertEquals("111111", n("lLiI11"))
        assertEquals(null, n("0X1D8"))
        assertEquals(null, n("0X1D8CC"))
        assertEquals(null, n("0X1DUC"))
    }
}

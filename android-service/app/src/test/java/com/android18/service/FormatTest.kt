package com.android18.service

import com.android18.service.util.formatBytes
import com.android18.service.util.formatTime
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class FormatTest {

    @Test
    fun bytesBelowOneKiBArePlain() {
        assertEquals("0 B", formatBytes(0))
        assertEquals("512 B", formatBytes(512))
        assertEquals("1023 B", formatBytes(1023))
    }

    @Test
    fun unitsScaleWithOneDecimal() {
        assertEquals("1.0 KB", formatBytes(1024))
        assertEquals("1.5 MB", formatBytes(1_572_864))
        assertEquals("2.0 GB", formatBytes(2L * 1024 * 1024 * 1024))
    }

    @Test
    fun journalTimesUseHhMmSs() {
        // Locale/TZ independent: epoch formats as three zero-padded pairs.
        assertTrue(formatTime(0).matches(Regex("\\d{2}:\\d{2}:\\d{2}")))
    }
}

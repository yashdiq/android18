package com.android18.service.util

import com.android18.service.domain.model.JournalEntry

/** How recent a desktop request must be for the phone to call it connected. */
const val DESKTOP_ACTIVE_WINDOW_MS = 15_000L

/**
 * The client address of the desktop currently talking to this phone, or
 * null. The desktop pings `/info` every few seconds while connected, so a
 * successful authorized request inside [windowMs] means "connected"; the
 * unauthenticated `/pair-request` does not count.
 */
fun activeDesktop(
    entries: List<JournalEntry>,
    now: Long,
    windowMs: Long = DESKTOP_ACTIVE_WINDOW_MS,
): String? = entries.lastOrNull {
    it.status in 200..299 && !it.endpoint.startsWith("/pair-request") &&
        now - it.atMillis <= windowMs
}?.let { it.client ?: "desktop" }

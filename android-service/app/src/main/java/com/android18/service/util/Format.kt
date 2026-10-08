package com.android18.service.util

import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** Human byte sizes: `12 B`, `1.4 MB`, `2.0 GB`. */
fun formatBytes(bytes: Long): String = when {
    bytes < 1024 -> "$bytes B"
    bytes < 1024 * 1024 -> String.format(Locale.US, "%.1f KB", bytes / 1024.0)
    bytes < 1024L * 1024 * 1024 -> String.format(Locale.US, "%.1f MB", bytes / 1_048_576.0)
    else -> String.format(Locale.US, "%.1f GB", bytes / 1_073_741_824.0)
}

/** `14:22:05` — journal rows. */
fun formatTime(millis: Long): String =
    SimpleDateFormat("HH:mm:ss", Locale.US).format(Date(millis))

/** `Mar 3, 14:22` — file modified stamps. */
fun formatDate(millis: Long): String =
    SimpleDateFormat("MMM d, HH:mm", Locale.US).format(Date(millis))

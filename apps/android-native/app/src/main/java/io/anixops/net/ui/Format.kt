package io.anixops.net.ui

import java.text.DateFormat
import java.util.Date
import java.util.Locale

/** Binary units, as every client shows them (`subscription-format.ts` `formatBytes`). */
fun formatBytes(bytes: Long): String {
    val units = listOf("B", "KB", "MB", "GB", "TB", "PB")
    var value = bytes.toDouble()
    var unit = 0
    while (value >= 1024 && unit < units.lastIndex) {
        value /= 1024
        unit += 1
    }
    if (unit == 0) return "$bytes B"
    val digits = when {
        value >= 100 -> 0
        value >= 10 -> 1
        else -> 2
    }
    return String.format(Locale.ROOT, "%.${digits}f %s", value, units[unit])
}

fun formatRate(bytesPerSecond: Long): String = "${formatBytes(bytesPerSecond)}/s"

/** A day in the user's locale from Unix seconds. */
fun formatDay(unixSeconds: Long, locale: Locale): String =
    DateFormat.getDateInstance(DateFormat.MEDIUM, locale).format(Date(unixSeconds * 1000))

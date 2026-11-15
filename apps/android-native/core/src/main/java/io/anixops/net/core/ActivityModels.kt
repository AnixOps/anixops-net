package io.anixops.net.core

import kotlinx.serialization.Serializable

/** One line of the app's own log (`app_logs`); [timeMs] is 0 when the line has no time. */
@Serializable
data class LogEntry(
    val timeMs: Long = 0,
    val module: String = "",
    /** `info`, `warn` or `error`. */
    val level: String = "info",
    val message: String = "",
)

/** The log viewer as `view.logs` reads it: the modules present, the lines shown newest first, and the copy. */
@Serializable
data class LogsView(
    val modules: List<String> = emptyList(),
    val shown: List<LogEntry> = emptyList(),
    val text: String = "",
)

/** One bar of the traffic chart: an hour or a day from [start] (Unix seconds). */
@Serializable
data class TrafficBar(val start: Long, val bytes: Long)

/** A host or policy and the bytes it moved; the host `*` is all the hosts the core did not keep by name. */
@Serializable
data class TrafficShare(val name: String, val bytes: Long)

/** The traffic statistics card as `view.traffic_stats` reads it for a period. */
@Serializable
data class TrafficStatsView(
    val start: Long = 0,
    val upload: Long = 0,
    val download: Long = 0,
    val bars: List<TrafficBar> = emptyList(),
    val hosts: List<TrafficShare> = emptyList(),
    val policies: List<TrafficShare> = emptyList(),
)

/** The statistics for a period, and whether anything was recorded at all (in the last seven days). */
data class TrafficPage(val view: TrafficStatsView, val recorded: Boolean)

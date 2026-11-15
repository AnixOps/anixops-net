package io.anixops.net.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.anixops.net.core.Core
import io.anixops.net.core.CoreException
import io.anixops.net.core.LogsView
import io.anixops.net.core.TrafficPage
import java.util.TimeZone
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** The log viewer: its filters and what `view.logs` made of them; [logs] is null until read. */
data class LogsState(
    val level: String = "all",
    val module: String = "",
    val query: String = "",
    val logs: LogsView? = null,
)

/** Which bars or list the traffic card shows. */
enum class StatsView { Time, Hosts, Policies }

/** The traffic statistics card: the period, the view, and what `view.traffic_stats` read; null until read. */
data class TrafficState(
    val period: String = "today",
    val view: StatsView = StatsView.Time,
    val page: TrafficPage? = null,
)

/** A sentence for the snackbar, a cleared list, or the log text to put on the clipboard. */
sealed interface ActivityEvent {
    data class Message(val text: String) : ActivityEvent

    data object LogsCleared : ActivityEvent

    data object StatsCleared : ActivityEvent

    data class CopyLogs(val text: String, val lines: Int) : ActivityEvent
}

class ActivityViewModel(private val core: Core) : ViewModel() {
    private val _logs = MutableStateFlow(LogsState())
    val logs: StateFlow<LogsState> = _logs.asStateFlow()

    private val _traffic = MutableStateFlow(TrafficState())
    val traffic: StateFlow<TrafficState> = _traffic.asStateFlow()

    private val _events = Channel<ActivityEvent>(Channel.BUFFERED)
    val events: Flow<ActivityEvent> = _events.receiveAsFlow()

    /** Reads the log every [LOGS_MILLIS] until cancelled; the logs page runs it while it shows. */
    suspend fun pollLogs() {
        while (true) {
            readLogs()
            delay(LOGS_MILLIS)
        }
    }

    /** Reads the statistics every [TRAFFIC_MILLIS] until cancelled (the client adds to them every 5 s). */
    suspend fun pollTraffic() {
        while (true) {
            readTraffic()
            delay(TRAFFIC_MILLIS)
        }
    }

    fun setLevel(level: String) = filterLogs { it.copy(level = level) }

    fun setModule(module: String) = filterLogs { it.copy(module = module) }

    fun setQuery(query: String) = filterLogs { it.copy(query = query) }

    fun refreshLogs() = viewModelScope.launch { readLogs() }

    fun copyLogs() = viewModelScope.launch {
        val logs = _logs.value.logs ?: return@launch
        if (logs.shown.isNotEmpty()) _events.send(ActivityEvent.CopyLogs(logs.text, logs.shown.size))
    }

    fun clearLogs() = viewModelScope.launch {
        if (guard { core.clearLogs() } != null) _events.send(ActivityEvent.LogsCleared)
        readLogs()
    }

    fun setPeriod(period: String) {
        _traffic.update { it.copy(period = period) }
        viewModelScope.launch { readTraffic() }
    }

    fun setView(view: StatsView) = _traffic.update { it.copy(view = view) }

    fun clearTraffic() = viewModelScope.launch {
        if (guard { core.clearTrafficStats() } != null) _events.send(ActivityEvent.StatsCleared)
        readTraffic()
    }

    private fun filterLogs(change: (LogsState) -> LogsState) {
        _logs.update(change)
        viewModelScope.launch { readLogs() }
    }

    private suspend fun readLogs() {
        val filters = _logs.value
        val logs = quiet { core.logs(filters.level, filters.module, filters.query) } ?: return
        // A filter changed while this read ran: the next read answers for it.
        _logs.update { if (it.level == filters.level && it.module == filters.module && it.query == filters.query) it.copy(logs = logs) else it }
    }

    private suspend fun readTraffic() {
        val period = _traffic.value.period
        val now = System.currentTimeMillis()
        val offset = TimeZone.getDefault().getOffset(now) / MILLIS
        val page = quiet { core.trafficStats(period, now / MILLIS, offset) } ?: return
        _traffic.update { if (it.period == period) it.copy(page = page) else it }
    }

    private suspend fun <T> guard(command: suspend () -> T): T? = try {
        command()
    } catch (error: CoreException) {
        _events.send(ActivityEvent.Message(error.message.orEmpty()))
        null
    }

    /** A poll that fails keeps what the page shows rather than repeating the refusal every few seconds. */
    private suspend fun <T> quiet(read: suspend () -> T): T? = try {
        read()
    } catch (error: CoreException) {
        null
    }

    companion object {
        private const val LOGS_MILLIS = 5_000L
        private const val TRAFFIC_MILLIS = 10_000L
        private const val MILLIS = 1_000
    }
}

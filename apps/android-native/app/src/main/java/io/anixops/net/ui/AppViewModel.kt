package io.anixops.net.ui

import android.content.SharedPreferences
import androidx.core.content.edit
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.anixops.net.core.ConnectionSnapshot
import io.anixops.net.core.Core
import io.anixops.net.core.CoreException
import io.anixops.net.core.NetworkProbe
import io.anixops.net.core.OperationResult
import io.anixops.net.core.Overview
import io.anixops.net.core.RuntimeSnapshot
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/**
 * What the overview page shows: the client's last snapshot and its `view.overview` reading, the open
 * connections, the exit seen from outside, whether a command is running, and the last refusal.
 */
data class UiState(
    val snapshot: RuntimeSnapshot? = null,
    val overview: Overview? = null,
    val connections: List<ConnectionSnapshot> = emptyList(),
    val probe: NetworkProbe? = null,
    val busy: Boolean = false,
    val notice: String? = null,
)

/** Proxy (connect and route) or debug (capture and rewrite), as on the other clients' 使用方式 switch. */
enum class AppMode(val key: String) {
    Proxy("proxy"),
    Debug("debug"),
    ;

    companion object {
        fun of(key: String?): AppMode = entries.firstOrNull { it.key == key } ?: Proxy
    }
}

class AppViewModel(
    private val core: Core,
    private val preferences: SharedPreferences,
) : ViewModel() {
    private val _state = MutableStateFlow(UiState())
    val state: StateFlow<UiState> = _state.asStateFlow()

    private val _mode = MutableStateFlow(AppMode.of(preferences.getString(MODE_KEY, null)))
    val mode: StateFlow<AppMode> = _mode.asStateFlow()

    private val _messages = Channel<String>(Channel.BUFFERED)

    /** What a command answered, for a snackbar. */
    val messages: Flow<String> = _messages.receiveAsFlow()

    /** Reads the overview every [POLL_MILLIS] until cancelled; the screen runs it while it is started. */
    suspend fun poll() {
        while (true) {
            refresh()
            delay(POLL_MILLIS)
        }
    }

    fun connect() = perform { core.connect() }

    fun disconnect() = perform { core.disconnect() }

    fun setRoutingMode(mode: String) = perform(report = true) { core.setRoutingMode(mode) }

    fun closeConnection(id: Long) = viewModelScope.launch {
        guard { core.closeConnection(id) }
        refresh()
    }

    fun closeAllConnections() = viewModelScope.launch {
        guard { core.closeAllConnections() }
        refresh()
    }

    fun showNotice(text: String) = _state.update { it.copy(notice = text) }

    fun setMode(mode: AppMode) {
        preferences.edit { putString(MODE_KEY, mode.key) }
        _mode.value = mode
    }

    private suspend fun refresh() {
        val data = guard { core.overview() } ?: return
        val connected = data.overview.state == "connected"
        val wasConnected = _state.value.overview?.state == "connected"
        _state.update {
            it.copy(
                snapshot = data.snapshot,
                overview = data.overview,
                connections = data.connections,
                probe = if (connected) it.probe else null,
            )
        }
        // The exit address once per connection: it costs a request through the tunnel.
        if (connected && !wasConnected) {
            viewModelScope.launch {
                val probe = guard { core.networkProbe(connected = true) }
                _state.update { if (it.overview?.state == "connected") it.copy(probe = probe) else it }
            }
        }
    }

    private fun perform(report: Boolean = false, command: suspend () -> OperationResult) {
        if (_state.value.busy) return
        _state.update { it.copy(busy = true, notice = null) }
        viewModelScope.launch {
            try {
                val result = command()
                if (report) _messages.send(result.message)
            } catch (error: CoreException) {
                _state.update { it.copy(notice = error.message) }
            } finally {
                refresh()
                _state.update { it.copy(busy = false) }
            }
        }
    }

    private suspend fun <T> guard(read: suspend () -> T): T? = try {
        read()
    } catch (error: CoreException) {
        _state.update { it.copy(notice = error.message) }
        null
    }

    companion object {
        const val PREFERENCES = "anixops"
        private const val MODE_KEY = "app-mode"
        private const val POLL_MILLIS = 1_000L
    }
}

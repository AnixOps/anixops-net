package io.anixops.net.ui

import android.content.SharedPreferences
import androidx.core.content.edit
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.anixops.net.core.Core
import io.anixops.net.core.CoreException
import io.anixops.net.core.OperationResult
import io.anixops.net.core.RuntimeSnapshot
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** What the screens show: the client's last snapshot, whether a command is running, and the last refusal. */
data class UiState(
    val snapshot: RuntimeSnapshot? = null,
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

    /** Reads the snapshot every [POLL_MILLIS] until cancelled; the screen runs it while it is started. */
    suspend fun poll() {
        while (true) {
            refresh()
            delay(POLL_MILLIS)
        }
    }

    fun connect() = perform { core.connect() }

    fun disconnect() = perform { core.disconnect() }

    fun showNotice(text: String) = _state.update { it.copy(notice = text) }

    fun setMode(mode: AppMode) {
        preferences.edit { putString(MODE_KEY, mode.key) }
        _mode.value = mode
    }

    private suspend fun refresh() {
        try {
            val snapshot = core.snapshot()
            _state.update { it.copy(snapshot = snapshot) }
        } catch (error: CoreException) {
            _state.update { it.copy(notice = error.message) }
        }
    }

    private fun perform(command: suspend () -> OperationResult) {
        if (_state.value.busy) return
        _state.update { it.copy(busy = true, notice = null) }
        viewModelScope.launch {
            try {
                command()
            } catch (error: CoreException) {
                _state.update { it.copy(notice = error.message) }
            } finally {
                refresh()
                _state.update { it.copy(busy = false) }
            }
        }
    }

    companion object {
        const val PREFERENCES = "anixops"
        private const val MODE_KEY = "app-mode"
        private const val POLL_MILLIS = 1_000L
    }
}

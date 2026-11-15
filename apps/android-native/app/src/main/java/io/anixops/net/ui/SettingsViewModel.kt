package io.anixops.net.ui

import androidx.annotation.StringRes
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.anixops.net.R
import io.anixops.net.core.Core
import io.anixops.net.core.CoreException
import io.anixops.net.core.HealthStatus
import io.anixops.net.core.NotificationSettings
import io.anixops.net.core.OnDemandSettings
import io.anixops.net.core.WifiRule
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** What the settings page reads from the client; each part is null until read. */
data class SettingsState(
    val notifications: NotificationSettings? = null,
    val onDemand: OnDemandSettings? = null,
    val health: HealthStatus? = null,
    /** The Wi-Fi name the 当前网络 button filled in, for the add field. */
    val currentSsid: String? = null,
    val busy: Boolean = false,
)

/** The three notification switches, in the order every client lists them. */
enum class NotificationKind { Scripts, Connection, Subscriptions }

/** A sentence for the snackbar: what the client answered, or one of the app's own. */
sealed interface SettingsEvent {
    data class Message(val text: String) : SettingsEvent

    data class Resource(@param:StringRes val id: Int) : SettingsEvent
}

class SettingsViewModel(private val core: Core) : ViewModel() {
    private val _state = MutableStateFlow(SettingsState())
    val state: StateFlow<SettingsState> = _state.asStateFlow()

    private val _events = Channel<SettingsEvent>(Channel.BUFFERED)
    val events: Flow<SettingsEvent> = _events.receiveAsFlow()

    /**
     * Reads the page, then the health status every [POLL_MILLIS] until cancelled (the install check finishes in
     * the background shortly after start). [language] is the client's code for the texts of notifications.
     */
    suspend fun poll(language: String) {
        val notifications = guard { core.notificationSettings(language) }
        val onDemand = guard { core.onDemandSettings() }
        _state.update { it.copy(notifications = notifications, onDemand = onDemand) }
        while (true) {
            val health = quiet { core.health() }
            if (health != null) _state.update { it.copy(health = health) }
            delay(POLL_MILLIS)
        }
    }

    fun toggleNotification(kind: NotificationKind) {
        val current = _state.value.notifications ?: return
        val next = when (kind) {
            NotificationKind.Scripts -> current.copy(scripts = !current.scripts)
            NotificationKind.Connection -> current.copy(connection = !current.connection)
            NotificationKind.Subscriptions -> current.copy(subscriptions = !current.subscriptions)
        }
        perform {
            val saved = core.notificationSettings(current.language, next)
            _state.update { it.copy(notifications = saved) }
            val on = when (kind) {
                NotificationKind.Scripts -> saved.scripts
                NotificationKind.Connection -> saved.connection
                NotificationKind.Subscriptions -> saved.subscriptions
            }
            SettingsEvent.Resource(if (on) R.string.notifications_turned_on else R.string.notifications_turned_off)
        }
    }

    fun setOnDemandEnabled(enabled: Boolean) = saveOnDemand(
        { it.copy(enabled = enabled) },
        if (enabled) R.string.wifi_rules_on else R.string.wifi_rules_off,
    )

    /** Adds [ssid] as a 断开 rule, as the other clients do; a name already listed or too long is ignored. */
    fun addWifiRule(ssid: String) {
        val name = ssid.trim()
        val settings = _state.value.onDemand ?: return
        if (!ssidFits(name) || settings.wifi.any { it.ssid == name }) return
        _state.update { it.copy(currentSsid = null) }
        saveOnDemand({ it.copy(wifi = it.wifi + WifiRule(name, "disconnect")) }, R.string.wifi_rule_added)
    }

    fun setWifiRule(ssid: String, action: String) = saveOnDemand(
        { settings -> settings.copy(wifi = settings.wifi.map { if (it.ssid == ssid) it.copy(action = action) else it }) },
        R.string.wifi_rules_saved,
    )

    fun removeWifiRule(ssid: String) =
        saveOnDemand({ settings -> settings.copy(wifi = settings.wifi.filterNot { it.ssid == ssid }) }, R.string.wifi_rule_removed)

    fun setOtherWifi(action: String) = saveOnDemand({ it.copy(otherWifi = action) }, R.string.wifi_rules_saved)

    fun setCellular(action: String) = saveOnDemand({ it.copy(cellular = action) }, R.string.wifi_rules_saved)

    fun setOther(action: String) = saveOnDemand({ it.copy(other = action) }, R.string.wifi_rules_saved)

    /** Fills the add field with the current Wi-Fi name; the page has asked for the location permission first. */
    fun fillCurrentNetwork() = perform {
        val network = core.currentNetwork()
        val ssid = network.ssid
        when {
            network.kind != "wifi" -> SettingsEvent.Resource(R.string.wifi_not_on_wifi)
            ssid.isNullOrEmpty() -> SettingsEvent.Resource(R.string.wifi_name_withheld)
            else -> {
                _state.update { it.copy(currentSsid = ssid) }
                SettingsEvent.Resource(R.string.wifi_name_filled)
            }
        }
    }

    fun validateConfiguration() = perform { SettingsEvent.Message(core.validateConfiguration().message) }

    fun restartCore() = perform { SettingsEvent.Message(core.restartCore().message) }

    fun setCrashReports(enabled: Boolean) = perform {
        val result = core.setCrashReports(enabled)
        quiet { core.health() }?.let { health -> _state.update { it.copy(health = health) } }
        SettingsEvent.Message(result.message)
    }

    fun createDiagnostics() = perform { SettingsEvent.Message(core.createDiagnostics().message) }

    private fun saveOnDemand(change: (OnDemandSettings) -> OnDemandSettings, @StringRes done: Int) {
        val current = _state.value.onDemand ?: return
        perform {
            val saved = core.onDemandSettings(change(current))
            _state.update { it.copy(onDemand = saved) }
            SettingsEvent.Resource(done)
        }
    }

    private fun perform(command: suspend () -> SettingsEvent) {
        if (_state.value.busy) return
        _state.update { it.copy(busy = true) }
        viewModelScope.launch {
            try {
                _events.send(command())
            } catch (error: CoreException) {
                _events.send(SettingsEvent.Message(error.message.orEmpty()))
            } finally {
                _state.update { it.copy(busy = false) }
            }
        }
    }

    private suspend fun <T> guard(read: suspend () -> T): T? = try {
        read()
    } catch (error: CoreException) {
        _events.send(SettingsEvent.Message(error.message.orEmpty()))
        null
    }

    private suspend fun <T> quiet(read: suspend () -> T): T? = try {
        read()
    } catch (error: CoreException) {
        null
    }

    companion object {
        private const val POLL_MILLIS = 5_000L

        /** Longest Wi-Fi name (IEEE 802.11: 32 bytes), as the client checks. */
        private const val MAX_SSID_BYTES = 32

        fun ssidFits(ssid: String): Boolean = ssid.toByteArray(Charsets.UTF_8).size in 1..MAX_SSID_BYTES
    }
}

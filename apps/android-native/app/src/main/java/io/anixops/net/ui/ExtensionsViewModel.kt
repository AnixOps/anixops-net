package io.anixops.net.ui

import android.content.SharedPreferences
import androidx.core.content.edit
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.anixops.net.core.Core
import io.anixops.net.core.CoreException
import io.anixops.net.core.MitmPlugin
import io.anixops.net.core.MitmPluginDetails
import io.anixops.net.core.OperationResult
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
 * What 扩展 (改写 in debug mode) reads beyond the snapshot: the plugins and the domains decrypted one by one.
 * [consented] says the user has confirmed once that decryption is only for traffic they may see (TERMS.md §4).
 */
data class ExtensionsState(
    val plugins: List<MitmPlugin> = emptyList(),
    val hosts: List<String> = emptyList(),
    val loaded: Boolean = false,
    val busy: Boolean = false,
    val consented: Boolean = false,
    /** The plugin whose contents are open, with what the engine read of it. */
    val inspected: Pair<MitmPlugin, MitmPluginDetails>? = null,
)

class ExtensionsViewModel(private val core: Core, private val preferences: SharedPreferences) : ViewModel() {
    private val _state = MutableStateFlow(ExtensionsState(consented = preferences.contains(CONSENT_KEY)))
    val state: StateFlow<ExtensionsState> = _state.asStateFlow()

    private val _messages = Channel<String>(Channel.BUFFERED)

    /** What commands answered, for a snackbar. */
    val messages: Flow<String> = _messages.receiveAsFlow()

    /** Reads the page every [POLL_MILLIS] until cancelled: plugins update scripts and a request can add a domain. */
    suspend fun poll() {
        while (true) {
            refresh()
            delay(POLL_MILLIS)
        }
    }

    /** Kept once per installation, as the other clients do; any stored value counts. */
    fun confirmDecryption() {
        preferences.edit { putLong(CONSENT_KEY, System.currentTimeMillis()) }
        _state.update { it.copy(consented = true) }
    }

    fun enableDecryption() = perform { core.enableHttpsMitm() }

    fun disableDecryption() = perform { core.disableHttpsMitm() }

    fun addHost(host: String) = perform { core.addDecryptHost(host.trim()) }

    fun removeHost(host: String) = perform { core.removeDecryptHost(host) }

    fun importPlugin(url: String) = perform { core.importMitmPlugin(url.trim()) }

    fun importPluginText(text: String, source: String) = perform { core.importMitmPluginText(text, source) }

    fun setPluginEnabled(id: String, enabled: Boolean) = perform { core.setMitmPluginEnabled(id, enabled) }

    fun removePlugin(id: String) = perform { core.removeMitmPlugin(id) }

    fun inspect(plugin: MitmPlugin) = viewModelScope.launch {
        try {
            val details = core.mitmPluginDetails(plugin.id)
            _state.update { it.copy(inspected = plugin to details) }
        } catch (error: CoreException) {
            _messages.send(error.message.orEmpty())
        }
    }

    fun closeInspector() = _state.update { it.copy(inspected = null) }

    /** Quiet: a poll that fails keeps what the page shows rather than repeating the refusal. */
    private suspend fun refresh() {
        try {
            val plugins = core.mitmPlugins()
            val hosts = core.decryptHosts()
            _state.update { it.copy(plugins = plugins, hosts = hosts, loaded = true) }
        } catch (error: CoreException) {
            _state.update { it.copy(loaded = true) }
        }
    }

    private fun perform(command: suspend () -> OperationResult) {
        if (_state.value.busy) return
        _state.update { it.copy(busy = true) }
        viewModelScope.launch {
            try {
                _messages.send(command().message)
            } catch (error: CoreException) {
                _messages.send(error.message.orEmpty())
            } finally {
                refresh()
                _state.update { it.copy(busy = false) }
            }
        }
    }

    companion object {
        private const val POLL_MILLIS = 5_000L
        const val CONSENT_KEY = "https-decryption-consent"
    }
}

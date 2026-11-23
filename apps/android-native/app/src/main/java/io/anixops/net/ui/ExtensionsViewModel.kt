package io.anixops.net.ui

import android.content.SharedPreferences
import androidx.core.content.edit
import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.anixops.net.core.Core
import io.anixops.net.core.CoreException
import io.anixops.net.core.DebugMapping
import io.anixops.net.core.DecryptDomain
import io.anixops.net.core.HeaderField
import io.anixops.net.core.MitmHostOutcome
import io.anixops.net.core.MitmPlugin
import io.anixops.net.core.MitmPluginDetails
import io.anixops.net.core.OperationResult
import io.anixops.net.core.RedirectDraft
import io.anixops.net.core.TrafficRedirect
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** The destination rewrite open in the editor: [id] null for a new one. */
data class RedirectEditor(val id: String?, val draft: RedirectDraft)

/**
 * What 扩展 (改写 in debug mode) reads beyond the snapshot: the plugins, the domains decrypted one by one, the
 * Map Local / Remote mappings and the destination rewrites.
 * [consented] says the user has confirmed once that decryption is only for traffic they may see (TERMS.md §4).
 */
data class ExtensionsState(
    val plugins: List<MitmPlugin> = emptyList(),
    /** The decryption list's patterns that are on and not exclusions. */
    val hosts: List<String> = emptyList(),
    /** The whole decryption list: domains and wildcard patterns, exclusions, paused lines. */
    val domains: List<DecryptDomain> = emptyList(),
    /** The engine's per-domain decryption results while it runs (`mitm_hosts`), for the failure marks. */
    val outcomes: List<MitmHostOutcome> = emptyList(),
    val mappings: List<DebugMapping> = emptyList(),
    val redirects: List<TrafficRedirect> = emptyList(),
    val loaded: Boolean = false,
    val busy: Boolean = false,
    val consented: Boolean = false,
    /** The plugin whose contents are open, with what the engine read of it. */
    val inspected: Pair<MitmPlugin, MitmPluginDetails>? = null,
    /**
     * The open plugin is being looked at before it is turned on: after an import, which leaves a plugin off, or
     * when its switch is turned on. The dialog then offers to turn it on.
     */
    val reviewing: Boolean = false,
    /** The mapping open in its editor page; a new one has an empty id. */
    val editingMapping: DebugMapping? = null,
    val editingRedirect: RedirectEditor? = null,
)

/** A sentence for the snackbar: what the client answered, or one the app writes in its own language. */
sealed interface ExtensionsMessage {
    data class Text(val text: String) : ExtensionsMessage

    /** Decryption is on and the root certificate is in Downloads at [location]. */
    data class RootSaved(val location: String, val fingerprint: String?) : ExtensionsMessage

    /** Decryption is on, but the root certificate could not be saved ([error]). */
    data class RootNotSaved(val error: String) : ExtensionsMessage
}

class ExtensionsViewModel(private val core: Core, private val preferences: SharedPreferences) : ViewModel() {
    private val _state = MutableStateFlow(ExtensionsState(consented = preferences.contains(CONSENT_KEY)))
    val state: StateFlow<ExtensionsState> = _state.asStateFlow()

    private val _messages = Channel<ExtensionsMessage>(Channel.BUFFERED)

    /** What commands answered, for a snackbar. */
    val messages: Flow<ExtensionsMessage> = _messages.receiveAsFlow()

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

    /**
     * Turns decryption on. Android saves the root to Downloads: the message says where, and its fingerprint to
     * compare in the system's list, in the app's language.
     */
    fun enableDecryption() {
        if (_state.value.busy) return
        _state.update { it.copy(busy = true) }
        viewModelScope.launch {
            try {
                val result = core.enableHttpsMitm()
                val location = result.location
                val saveError = result.saveError
                _messages.send(
                    when {
                        location != null -> ExtensionsMessage.RootSaved(location, result.fingerprint)
                        saveError != null -> ExtensionsMessage.RootNotSaved(saveError)
                        else -> ExtensionsMessage.Text(result.message)
                    },
                )
            } catch (error: CoreException) {
                _messages.send(ExtensionsMessage.Text(error.message.orEmpty()))
            } finally {
                refresh()
                _state.update { it.copy(busy = false) }
            }
        }
    }

    fun disableDecryption() = perform { core.disableHttpsMitm() }

    fun addHost(host: String, exclude: Boolean = false) = perform { core.addDecryptHost(host.trim(), exclude) }

    fun editHost(pattern: String, host: String, exclude: Boolean) =
        perform { core.editDecryptHost(pattern, host.trim(), exclude) }

    fun setHostEnabled(pattern: String, enabled: Boolean) = perform { core.setDecryptHostEnabled(pattern, enabled) }

    fun removeHost(host: String) = perform { core.removeDecryptHost(host) }

    /** Forgets the engine's decryption results, so only new failures are marked. */
    fun clearOutcomes() = viewModelScope.launch {
        val outcomes = try {
            core.mitmHosts(clear = true).hosts
        } catch (error: CoreException) {
            emptyList()
        }
        _state.update { it.copy(outcomes = outcomes) }
    }

    /**
     * Imports a plugin, which the client saves off (feature-gap-05): nothing it holds runs until the user has looked
     * at its domains, rewrite lines and scripts, so the review opens right after.
     */
    fun importPlugin(url: String) = perform(onSuccess = ::reviewImported) { core.importMitmPlugin(url.trim()) }

    fun importPluginText(text: String, source: String) =
        perform(onSuccess = ::reviewImported) { core.importMitmPluginText(text, source) }

    /** Turning a plugin on opens the review first; turning one off is at once. */
    fun setPluginEnabled(id: String, enabled: Boolean) {
        if (enabled) {
            _state.value.plugins.firstOrNull { it.id == id }?.let { review(it) }
        } else {
            perform { core.setMitmPluginEnabled(id, false) }
        }
    }

    /** Opens [plugin] for the user to look at before turning it on. */
    fun review(plugin: MitmPlugin) = inspect(plugin, reviewing = true)

    /** Turns on the plugin the user has looked at, and closes the review once the client took it. */
    fun enableReviewed(id: String) =
        perform(onSuccess = { closeInspector() }) { core.setMitmPluginEnabled(id, true) }

    fun removePlugin(id: String) = perform { core.removeMitmPlugin(id) }

    fun inspect(plugin: MitmPlugin, reviewing: Boolean = false) = viewModelScope.launch {
        try {
            val details = core.mitmPluginDetails(plugin.id)
            _state.update { it.copy(inspected = plugin to details, reviewing = reviewing) }
        } catch (error: CoreException) {
            _messages.send(ExtensionsMessage.Text(error.message.orEmpty()))
        }
    }

    fun closeInspector() = _state.update { it.copy(inspected = null, reviewing = false) }

    /** Opens the review of the plugin an import left off, read afresh so that it shows what was just saved. */
    private fun reviewImported(result: OperationResult) {
        val id = pluginToReview(result) ?: return
        viewModelScope.launch {
            val plugin = try {
                core.mitmPlugins().firstOrNull { it.id == id }
            } catch (error: CoreException) {
                null
            } ?: return@launch
            if (!plugin.enabled) review(plugin)
        }
    }

    /**
     * The open mapping editor's fields, kept here from opening to saving or leaving so that a rotation or another page
     * does not lose what was typed.
     */
    var mappingDraft: MappingDraft? = null
        private set

    /** Opens the mapping editor on [entry], or on a new mapping answering JSON locally. */
    fun editMapping(entry: DebugMapping?) {
        val mapping = entry ?: NEW_MAPPING
        mappingDraft = MappingDraft(mapping)
        _state.update { it.copy(editingMapping = mapping) }
    }

    fun closeMappingEditor() {
        mappingDraft = null
        _state.update { it.copy(editingMapping = null) }
    }

    /** Saves the edited mapping and closes the editor once the core has taken it. */
    fun saveMapping(entry: DebugMapping) = perform(onSuccess = { closeMappingEditor() }) { core.saveDebugMapping(entry) }

    fun setMappingEnabled(entry: DebugMapping, enabled: Boolean) =
        perform { core.saveDebugMapping(entry.copy(enabled = enabled)) }

    fun removeMapping(id: String) = perform { core.removeDebugMapping(id) }

    fun editRedirect(entry: TrafficRedirect?) = _state.update {
        it.copy(editingRedirect = RedirectEditor(entry?.id, entry?.draft ?: RedirectDraft()))
    }

    fun closeRedirectEditor() = _state.update { it.copy(editingRedirect = null) }

    fun saveRedirect(id: String?, draft: RedirectDraft) =
        perform(onSuccess = { closeRedirectEditor() }) { core.addTrafficRedirect(draft, id) }

    fun setRedirectEnabled(id: String, enabled: Boolean) = perform {
        core.saveTrafficRedirects(_state.value.redirects.map { if (it.id == id) it.copy(enabled = enabled) else it })
    }

    fun removeRedirect(id: String) = perform { core.saveTrafficRedirects(_state.value.redirects.filter { it.id != id }) }

    /** Quiet: a poll that fails keeps what the page shows rather than repeating the refusal. */
    private suspend fun refresh() {
        try {
            val plugins = core.mitmPlugins()
            val domains = core.decryptDomains()
            val hosts = domains.filter { it.enabled && !it.exclude }.map { it.pattern }
            val mappings = core.debugMappings()
            val redirects = core.trafficRedirects()
            val outcomes = try {
                core.mitmHosts().hosts
            } catch (error: CoreException) {
                emptyList()
            }
            _state.update {
                it.copy(
                    plugins = plugins,
                    hosts = hosts,
                    domains = domains,
                    outcomes = outcomes,
                    mappings = mappings,
                    redirects = redirects,
                    loaded = true,
                )
            }
        } catch (error: CoreException) {
            _state.update { it.copy(loaded = true) }
        }
    }

    /**
     * Runs [command] once at a time; [onSuccess] runs with the answer when the core took it (an editor then closes,
     * an import opens its review).
     */
    private fun perform(onSuccess: (OperationResult) -> Unit = {}, command: suspend () -> OperationResult) {
        if (_state.value.busy) return
        _state.update { it.copy(busy = true) }
        viewModelScope.launch {
            try {
                val result = command()
                onSuccess(result)
                _messages.send(ExtensionsMessage.Text(result.message))
            } catch (error: CoreException) {
                _messages.send(ExtensionsMessage.Text(error.message.orEmpty()))
            } finally {
                refresh()
                _state.update { it.copy(busy = false) }
            }
        }
    }

    companion object {
        private const val POLL_MILLIS = 5_000L
        const val CONSENT_KEY = "https-decryption-consent"

        /** A new mapping, as the desktop editor starts one: Map Local answering JSON. */
        val NEW_MAPPING = DebugMapping(headers = listOf(HeaderField("Content-Type", "application/json; charset=utf-8")))
    }
}

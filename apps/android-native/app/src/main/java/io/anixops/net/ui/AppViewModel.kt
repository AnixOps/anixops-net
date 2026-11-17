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

/** What following an `anixops://` link came to, for a snackbar. */
sealed interface LinkEvent {
    /** The link was refused: [reason] is the shared view's id, [detail] what it names. */
    data class Refused(val reason: String, val detail: String?) : LinkEvent

    data class NoNode(val node: String) : LinkEvent

    data class NoMember(val policy: String, val member: String) : LinkEvent

    data object ImportOffered : LinkEvent
}

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
    val nodeCount: Int = 0,
)

/** Where debug mode's traffic leaves: straight out, or through the nodes and rules of proxy mode. */
enum class DebugExit(val key: String) {
    Direct("direct"),
    Node("node"),
    ;

    companion object {
        /** Direct until the user picks the nodes or imports some in debug mode. */
        fun of(key: String?): DebugExit = if (key == Node.key) Node else Direct
    }
}

/** The engine runs with no node in debug mode with the direct exit, and in debug mode before any node exists. */
fun wantsDirectEgress(mode: AppMode, exit: DebugExit, hasNodes: Boolean): Boolean =
    mode == AppMode.Debug && (exit == DebugExit.Direct || !hasNodes)

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

    private val _debugExit = MutableStateFlow(DebugExit.of(preferences.getString(EXIT_KEY, null)))
    val debugExit: StateFlow<DebugExit> = _debugExit.asStateFlow()

    /** The direct-exit value last sent, so that it is sent once per wanted value. */
    private var sentDirect: Boolean? = null

    /** The node count of the last read; null before the first. */
    private var knownNodes: Int? = null

    private val _termsAccepted = MutableStateFlow(preferences.getString(TERMS_KEY, null))

    /** The version of the user agreement accepted on this device, or null before the first start's consent. */
    val termsAccepted: StateFlow<String?> = _termsAccepted.asStateFlow()

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

    private val _links = Channel<LinkEvent>(Channel.BUFFERED)

    /** What following an `anixops://` link came to, when it is not a command's own answer. */
    val links: Flow<LinkEvent> = _links.receiveAsFlow()

    /**
     * Follows an `anixops://` link as every client does (`view.parse_app_link`). [connect] connects the way the
     * screen does, asking for the VPN consent first; [offerImport] fills in the import form, which saves nothing
     * until the user confirms.
     */
    fun followLink(raw: String, connect: () -> Unit, offerImport: (String) -> Unit) {
        viewModelScope.launch {
            val link = guard { core.parseAppLink(raw) } ?: return@launch
            val connected = _state.value.overview?.state == "connected"
            when (link.action) {
                "connect" -> if (!connected) connect()
                "disconnect" -> disconnect()
                "toggle" -> if (connected) disconnect() else connect()
                "mode" -> link.mode?.let(::setRoutingMode)
                "select" -> {
                    val name = link.node.orEmpty()
                    val node = guard { core.findLinkNode(core.nodes(), name, link.group) }
                    if (node != null) perform(report = true) { core.switchNode(node.id) } else _links.send(LinkEvent.NoNode(name))
                }
                "select_member" -> {
                    val policy = link.policy.orEmpty()
                    val member = link.member.orEmpty()
                    val target = guard { core.findLinkMember(core.nativeGroups(), policy, member) }
                    if (target != null) {
                        perform(report = true) { core.selectGroupOutbound(target.tag, target.outbound) }
                    } else {
                        _links.send(LinkEvent.NoMember(policy, member))
                    }
                }
                "import" -> link.url?.let {
                    offerImport(it)
                    _links.send(LinkEvent.ImportOffered)
                }
                else -> _links.send(LinkEvent.Refused(link.error ?: "unknown_action", link.detail))
            }
        }
    }

    fun connect() = perform { core.connect() }

    fun disconnect() = perform { core.disconnect() }

    fun setRoutingMode(mode: String) = perform(report = true) { core.setRoutingMode(mode) }

    fun setDnsPreset(preset: String) = perform(report = true) { core.setDnsPreset(preset) }

    fun setBlockQuic(enabled: Boolean) = perform(report = true) { core.setBlockQuic(enabled) }

    fun closeConnection(id: Long) = viewModelScope.launch {
        guard { core.closeConnection(id) }
        refresh()
    }

    fun closeAllConnections() = viewModelScope.launch {
        guard { core.closeAllConnections() }
        refresh()
    }

    fun showNotice(text: String) = _state.update { it.copy(notice = text) }

    fun clearNotice() = _state.update { it.copy(notice = null) }

    fun setMode(mode: AppMode) {
        preferences.edit { putString(MODE_KEY, mode.key) }
        _mode.value = mode
        viewModelScope.launch { refresh() }
    }

    fun setDebugExit(exit: DebugExit) {
        preferences.edit { putString(EXIT_KEY, exit.key) }
        _debugExit.value = exit
        viewModelScope.launch { refresh() }
    }

    fun acceptTerms(version: String) {
        preferences.edit { putString(TERMS_KEY, version) }
        _termsAccepted.value = version
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
                nodeCount = data.nodeCount,
            )
        }
        // Importing nodes in debug mode means they are wanted as the exit.
        if (knownNodes == 0 && data.nodeCount > 0 && _mode.value == AppMode.Debug) {
            preferences.edit { putString(EXIT_KEY, DebugExit.Node.key) }
            _debugExit.value = DebugExit.Node
        }
        knownNodes = data.nodeCount
        syncDirectEgress(data.snapshot.directEgress, data.nodeCount)
        // The exit address once per connection: it costs a request through the tunnel.
        if (connected && !wasConnected) {
            viewModelScope.launch {
                val probe = guard { core.networkProbe(connected = true) }
                _state.update { if (it.overview?.state == "connected") it.copy(probe = probe) else it }
            }
        }
    }

    /** Tells the backend [wantsDirectEgress]; it keeps the choice, so it is sent only when it differs. */
    private fun syncDirectEgress(current: Boolean, nodeCount: Int) {
        val want = wantsDirectEgress(_mode.value, _debugExit.value, hasNodes = nodeCount > 0)
        if (current == want || sentDirect == want) return
        sentDirect = want
        viewModelScope.launch { guard { core.setDirectEgress(want) } }
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
        private const val TERMS_KEY = "terms-accepted"
        private const val EXIT_KEY = "debug-exit"
        private const val POLL_MILLIS = 1_000L
    }
}

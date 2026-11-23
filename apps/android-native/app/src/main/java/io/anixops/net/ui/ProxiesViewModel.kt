package io.anixops.net.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.anixops.net.core.Core
import io.anixops.net.core.CoreException
import io.anixops.net.core.NodeDelay
import io.anixops.net.core.NodeCheckup
import io.anixops.net.core.NodeForm
import io.anixops.net.core.OperationResult
import io.anixops.net.core.ProxiesPage
import io.anixops.net.core.SubscriptionRow
import kotlinx.coroutines.Job
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch

/** What the proxies page shows: subscriptions, then the nodes as `view.proxies` reads them. */
data class ProxiesState(
    val subscriptions: List<SubscriptionRow> = emptyList(),
    val page: ProxiesPage = ProxiesPage(),
    val query: String = "",
    val testing: Boolean = false,
    val busy: Boolean = false,
    val loaded: Boolean = false,
    /** A link an `anixops://import` link brought, for the import form; null once the form took it. */
    val pendingImport: String? = null,
    /** The node checkup whose dialog is open; null when none is. */
    val checkup: CheckupState? = null,
)

/** The checkup of one node: whether it runs, what it found and why it did not work. */
data class CheckupState(
    val nodeId: String,
    val label: String,
    val running: Boolean = false,
    val result: NodeCheckup? = null,
    val problem: String? = null,
)

/** A message for the snackbar, or a share link to put on the clipboard. */
sealed interface ProxiesEvent {
    data class Message(val text: String) : ProxiesEvent

    data class ShareLink(val node: String, val link: String) : ProxiesEvent
}

class ProxiesViewModel(private val core: Core) : ViewModel() {
    private val _state = MutableStateFlow(ProxiesState())
    val state: StateFlow<ProxiesState> = _state.asStateFlow()

    private val _events = Channel<ProxiesEvent>(Channel.BUFFERED)
    val events: Flow<ProxiesEvent> = _events.receiveAsFlow()

    private var delays: Map<String, NodeDelay> = emptyMap()

    private var checking: Job? = null

    private var searching: Job? = null

    /** Reads the page every [POLL_MILLIS] until cancelled (subscriptions update in the background). */
    suspend fun poll() {
        while (true) {
            refresh()
            delay(POLL_MILLIS)
        }
    }

    /**
     * Shows the nodes matching [query] once typing pauses; a newer query cancels the read of an older one, and
     * [refresh] drops an answer for a query that is no longer the page's.
     */
    fun search(query: String) {
        _state.update { it.copy(query = query) }
        searching?.cancel()
        searching = viewModelScope.launch {
            delay(SEARCH_PAUSE_MILLIS)
            refresh()
        }
    }

    fun select(nodeId: String) = perform { core.switchNode(nodeId) }

    fun selectFastest() = perform { core.selectFastestNode() }

    /** Tests the nodes the page shows now, all at once. */
    fun testShown() {
        if (_state.value.testing) return
        val ids = _state.value.page.view.sections.flatMap { section -> section.nodes.map { it.id } }
        if (ids.isEmpty()) return
        _state.update { it.copy(testing = true) }
        viewModelScope.launch {
            try {
                val results = core.testDelays(ids)
                delays = delays + results.associateBy { it.nodeId }
            } catch (error: CoreException) {
                _events.send(ProxiesEvent.Message(error.message.orEmpty()))
            } finally {
                _state.update { it.copy(testing = false) }
                refresh()
            }
        }
    }

    fun share(nodeId: String, label: String) = viewModelScope.launch {
        try {
            _events.send(ProxiesEvent.ShareLink(label, core.shareLink(nodeId)))
        } catch (error: CoreException) {
            _events.send(ProxiesEvent.Message(error.message.orEmpty()))
        }
    }

    /** Opens the checkup dialog of a node; nothing runs until [runCheckup]. */
    fun openCheckup(nodeId: String, label: String) {
        closeCheckup()
        _state.update { it.copy(checkup = CheckupState(nodeId, label)) }
    }

    /** Closes the checkup dialog; the answer of a checkup still running is dropped. */
    fun closeCheckup() {
        checking?.cancel()
        checking = null
        _state.update { it.copy(checkup = null) }
    }

    /** Runs the checkup of the open dialog's node; [throughput] adds the download. */
    fun runCheckup(throughput: Boolean) {
        val open = _state.value.checkup ?: return
        if (open.running) return
        _state.update { it.copy(checkup = open.copy(running = true, problem = null)) }
        checking = viewModelScope.launch {
            val (result, problem) = try {
                core.nodeCheckup(open.nodeId, throughput) to null
            } catch (error: CoreException) {
                null to error.message.orEmpty()
            }
            _state.update { state ->
                val now = state.checkup
                if (now?.nodeId != open.nodeId) {
                    state
                } else {
                    state.copy(checkup = now.copy(running = false, result = result, problem = problem))
                }
            }
        }
    }

    /** The node's edit form, or null after telling the user why it cannot be read. */
    suspend fun form(nodeId: String): NodeForm? = try {
        core.nodeForm(nodeId)
    } catch (error: CoreException) {
        _events.send(ProxiesEvent.Message(error.message.orEmpty()))
        null
    }

    fun edit(nodeId: String, name: String, server: String, port: String) =
        perform { core.editNode(nodeId, name, server, port) }

    fun remove(nodeId: String) = perform { core.removeNode(nodeId) }

    /** Puts [link] in the import form; nothing is saved until the user confirms there. */
    fun offerImport(link: String) = _state.update { it.copy(pendingImport = link) }

    fun takeImport() = _state.update { it.copy(pendingImport = null) }

    fun importSubscription(location: String) = perform { core.importSubscription(location.trim()) }

    fun useSubscription(id: String) = perform { core.selectSubscription(id) }

    fun updateSubscription(id: String) = perform { core.updateSubscription(id) }

    fun renameSubscription(id: String, name: String) = perform { core.renameSubscription(id, name.trim()) }

    fun setInterval(id: String, hours: Int) = perform { core.setSubscriptionInterval(id, hours) }

    fun removeSubscription(id: String) = perform { core.removeSubscription(id) }

    private suspend fun refresh() {
        try {
            val subscriptions = core.subscriptions()
            val query = _state.value.query
            val page = core.proxies(query, delays)
            // Reads overlap (the poll, a search, a test): an answer for a query the user has typed past is not shown.
            _state.update {
                if (it.query != query) {
                    it.copy(subscriptions = subscriptions)
                } else {
                    it.copy(subscriptions = subscriptions, page = page, loaded = true)
                }
            }
        } catch (error: CoreException) {
            _events.send(ProxiesEvent.Message(error.message.orEmpty()))
        }
    }

    private fun perform(command: suspend () -> OperationResult) {
        if (_state.value.busy) return
        _state.update { it.copy(busy = true) }
        viewModelScope.launch {
            try {
                _events.send(ProxiesEvent.Message(command().message))
            } catch (error: CoreException) {
                _events.send(ProxiesEvent.Message(error.message.orEmpty()))
            } finally {
                refresh()
                _state.update { it.copy(busy = false) }
            }
        }
    }

    companion object {
        private const val POLL_MILLIS = 3_000L

        /** How long typing must pause before the search runs. */
        const val SEARCH_PAUSE_MILLIS = 250L

        /** The automatic update intervals offered, in hours; 0 is by hand (`subscription-format.ts`). */
        val INTERVALS = listOf(0, 1, 3, 6, 12, 24, 48, 72, 168)
    }
}

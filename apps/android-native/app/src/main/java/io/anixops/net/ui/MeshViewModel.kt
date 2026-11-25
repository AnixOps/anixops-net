package io.anixops.net.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.anixops.net.core.Core
import io.anixops.net.core.CoreException
import io.anixops.net.core.MeshNetworkDraft
import io.anixops.net.core.MeshPage
import io.anixops.net.core.MeshRefusal
import io.anixops.net.core.MeshRouting
import io.anixops.net.core.MeshSavedNetwork
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonArray

/**
 * The 组网 (mesh) entry and page: the engine's status as the shared views show it, and how the member list is
 * grouped, filtered, sorted and searched. [page] is null while the engine is not running or has no mesh module; the
 * entry is then hidden (docs/mesh-ui.md, 入口与导航).
 */
data class MeshState(
    val page: MeshPage? = null,
    /** `region`, `role` or `none`. */
    val groupBy: String = "region",
    /** `all`, `online`, `unreachable`, `gateway`, `host`, `border` or `custom_path`. */
    val filter: String = "all",
    /** `name`, `latency` or `address`. */
    val sort: String = "name",
    val query: String = "",
    /** Why the last join, leave or route change did not take effect. */
    val refusal: MeshRefusal? = null,
    val busy: Boolean = false,
    /** The networks saved on this device, without secrets. */
    val saved: List<MeshSavedNetwork> = emptyList(),
    /** The custom paths before the last removal, for 撤销 (T0); null when there is nothing to undo. */
    val removedPaths: JsonArray? = null,
)

/** What the mesh entry and page can ask for. */
data class MeshActions(
    val setJoined: (Boolean) -> Unit = {},
    val setGroupBy: (String) -> Unit = {},
    val setFilter: (String) -> Unit = {},
    val setSort: (String) -> Unit = {},
    val setQuery: (String) -> Unit = {},
    val setSubnetRoute: (cidr: String, route: String) -> Unit = { _, _ -> },
    val dismissRefusal: () -> Unit = {},
    /** Saves the 添加网络 form and joins the network; the answer comes back as [MeshState.refusal]. */
    val addNetwork: (MeshNetworkDraft) -> Unit = {},
    /** Removes the network the page shows, after the name was confirmed. */
    val removeNetwork: () -> Unit = {},
    /** The default path policy: `latency` or `hops`. */
    val setPolicy: (String) -> Unit = {},
    /** Removes a custom path, by id; [undoPathRemoval] puts the list back. */
    val removePath: (String) -> Unit = {},
    val undoPathRemoval: () -> Unit = {},
    /** Adds a custom path; false when a member on it has no virtual address. */
    val addPath: (MeshRouting.Draft) -> Boolean = { false },
)

class MeshViewModel(private val core: Core) : ViewModel() {
    private val _state = MutableStateFlow(MeshState())
    val state: StateFlow<MeshState> = _state.asStateFlow()

    /** Reads the status every [POLL_MILLIS] (2 s in the foreground) until cancelled. */
    suspend fun poll() {
        while (true) {
            refresh()
            delay(POLL_MILLIS)
        }
    }

    fun setJoined(joined: Boolean) {
        val network = networkId() ?: return
        perform { core.setMeshJoined(network, joined) }
    }

    fun addNetwork(draft: MeshNetworkDraft) {
        val config = draft.config() ?: return
        perform { core.saveMeshNetwork(config) }
    }

    fun removeNetwork() {
        val network = networkId() ?: return
        perform { core.removeMeshNetwork(network) }
    }

    fun setSubnetRoute(cidr: String, route: String) {
        val network = networkId() ?: return
        perform { core.setMeshSubnetRoute(network, cidr, route) }
    }

    fun setPolicy(policy: String) {
        val network = networkId() ?: return
        perform { core.setMeshPolicy(network, policy) }
    }

    fun removePath(id: String) {
        val network = networkId() ?: return
        val status = _state.value.page?.status
        val before = MeshRouting.paths(status) ?: return
        val after = MeshRouting.paths(status, remove = id) ?: return
        perform {
            core.setMeshPaths(network, after).also { refusal ->
                _state.update { it.copy(removedPaths = if (refusal == null) before else null) }
            }
        }
    }

    fun undoPathRemoval() {
        val network = networkId() ?: return
        val paths = _state.value.removedPaths ?: return
        _state.update { it.copy(removedPaths = null) }
        perform { core.setMeshPaths(network, paths) }
    }

    fun addPath(draft: MeshRouting.Draft): Boolean {
        val network = networkId() ?: return false
        val paths = MeshRouting.paths(_state.value.page?.status, add = draft) ?: return false
        _state.update { it.copy(removedPaths = null) }
        perform { core.setMeshPaths(network, paths) }
        return true
    }

    fun setGroupBy(value: String) = reread { it.copy(groupBy = value) }

    fun setFilter(value: String) = reread { it.copy(filter = value) }

    fun setSort(value: String) = reread { it.copy(sort = value) }

    fun setQuery(value: String) = reread { it.copy(query = value) }

    fun dismissRefusal() = _state.update { it.copy(refusal = null) }

    private suspend fun refresh() {
        val current = _state.value
        // An answer the app cannot read hides the entry, as an engine without the mesh module does.
        val page = try {
            core.meshPage(current.groupBy, current.filter, current.sort, current.query)
        } catch (error: CoreException) {
            null
        }
        val saved = try {
            core.meshNetworks()
        } catch (error: CoreException) {
            _state.value.saved
        }
        _state.update { it.copy(page = page, saved = saved) }
    }

    private fun networkId(): String? = _state.value.page?.head?.network?.id

    private fun reread(change: (MeshState) -> MeshState) {
        _state.update(change)
        viewModelScope.launch { refresh() }
    }

    private fun perform(command: suspend () -> MeshRefusal?) {
        if (_state.value.busy) return
        _state.update { it.copy(busy = true) }
        viewModelScope.launch {
            val refusal = command()
            _state.update { it.copy(refusal = refusal, busy = false) }
            refresh()
        }
    }

    companion object {
        const val POLL_MILLIS = 2_000L
    }
}

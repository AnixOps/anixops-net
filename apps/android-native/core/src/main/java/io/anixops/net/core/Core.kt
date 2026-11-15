package io.anixops.net.core

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.KSerializer
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

/**
 * Typed commands over the client. Every call runs on [dispatcher] (commands may block for long: downloads,
 * delay tests, the VPN consent) and throws [CoreException] when the client refuses. Names and arguments are the
 * shared interface's (`apps/windows-gui/ui/src/bridge.ts`); page logic is the client's `view.*` commands
 * (crates/client-view), so every client shows the same thing.
 */
class Core(
    private val transport: CoreTransport,
    private val dispatcher: CoroutineDispatcher = Dispatchers.IO,
) {
    suspend fun snapshot(): RuntimeSnapshot = call("runtime_snapshot", RuntimeSnapshot.serializer())

    suspend fun connect(): OperationResult = call("connect", OperationResult.serializer())

    suspend fun disconnect(): OperationResult = call("disconnect", OperationResult.serializer())

    // The overview page.

    /** The snapshot, the node list and (while connected) the traffic counters, read as the overview page. */
    suspend fun overview(): OverviewData {
        val snapshot = raw("runtime_snapshot")
        val nodes = raw("list_nodes")
        val connected = decode(RuntimeSnapshot.serializer(), snapshot).state == ConnectionState.Connected
        // The engine answers its counters only while it runs; a race with a disconnect reads as no traffic.
        val traffic = if (connected) orNull { control("traffic") } ?: JsonNull else JsonNull
        val overview = call(
            "view.overview",
            Overview.serializer(),
            buildJsonObject {
                put("snapshot", snapshot)
                put("nodes", nodes)
                put("traffic", traffic)
            },
        )
        val connections = if (connected) {
            orNull { decode(ConnectionsSnapshot.serializer(), control("connections")) }
        } else {
            null
        }
        return OverviewData(decode(RuntimeSnapshot.serializer(), snapshot), overview, connections?.active.orEmpty())
    }

    suspend fun setRoutingMode(mode: String): OperationResult =
        call("set_routing_mode", OperationResult.serializer(), args("mode" to JsonPrimitive(mode)))

    suspend fun closeConnection(id: Long) {
        controlRequest(buildJsonObject { put("cmd", "close_connection"); put("id", id) })
    }

    suspend fun closeAllConnections() {
        controlRequest(buildJsonObject { put("cmd", "close_all") })
    }

    suspend fun networkProbe(connected: Boolean): NetworkProbe =
        call("network_probe", NetworkProbe.serializer(), args("connected" to JsonPrimitive(connected)))

    // The proxies page.

    suspend fun nodes(): List<NodeSummary> = call("list_nodes", ListSerializer(NodeSummary.serializer()))

    /** The node list as the proxies page shows it for [query], with the last [delays]. */
    suspend fun proxies(query: String, delays: Map<String, NodeDelay>): ProxiesPage {
        val nodes = raw("list_nodes")
        val view = call(
            "view.proxies",
            ProxiesView.serializer(),
            buildJsonObject {
                put("nodes", nodes)
                putJsonObject("delays") {
                    delays.forEach { (id, delay) -> put(id, Envelope.json.encodeToJsonElement(NodeDelay.serializer(), delay)) }
                }
                put("query", query)
            },
        )
        val editable = decode(ListSerializer(NodeSummary.serializer()), nodes, "list_nodes")
            .filter { it.editable }
            .mapTo(mutableSetOf()) { it.id }
        return ProxiesPage(view, editable)
    }

    suspend fun switchNode(nodeId: String): OperationResult =
        call("switch_node", OperationResult.serializer(), args("nodeId" to JsonPrimitive(nodeId)))

    suspend fun testDelays(nodeIds: List<String>, url: String = DELAY_URL): List<NodeDelay> = call(
        "test_node_delays",
        ListSerializer(NodeDelay.serializer()),
        buildJsonObject {
            putJsonArray("nodeIds") { nodeIds.forEach { add(JsonPrimitive(it)) } }
            put("url", url)
        },
    )

    suspend fun selectFastestNode(): OperationResult = call("select_fastest_node", OperationResult.serializer())

    suspend fun shareLink(nodeId: String): String =
        call("node_share_link", String.serializer(), args("nodeId" to JsonPrimitive(nodeId)))

    suspend fun nodeForm(nodeId: String): NodeForm =
        call("node_form", NodeForm.serializer(), args("nodeId" to JsonPrimitive(nodeId)))

    suspend fun editNode(nodeId: String, name: String, server: String, port: String): OperationResult = call(
        "edit_node",
        OperationResult.serializer(),
        args(
            "nodeId" to JsonPrimitive(nodeId),
            "name" to JsonPrimitive(name),
            "server" to JsonPrimitive(server),
            "port" to JsonPrimitive(port),
        ),
    )

    suspend fun removeNode(nodeId: String): OperationResult =
        call("remove_node", OperationResult.serializer(), args("nodeId" to JsonPrimitive(nodeId)))

    suspend fun subscriptions(): List<SubscriptionSummary> =
        call("list_subscriptions", ListSerializer(SubscriptionSummary.serializer()))

    suspend fun importSubscription(location: String): OperationResult =
        call("import_subscription", OperationResult.serializer(), args("location" to JsonPrimitive(location)))

    suspend fun selectSubscription(id: String): OperationResult =
        call("select_subscription", OperationResult.serializer(), args("id" to JsonPrimitive(id)))

    suspend fun updateSubscription(id: String): OperationResult =
        call("update_subscription_source", OperationResult.serializer(), args("id" to JsonPrimitive(id)))

    suspend fun renameSubscription(id: String, name: String): OperationResult = call(
        "rename_subscription",
        OperationResult.serializer(),
        args("id" to JsonPrimitive(id), "name" to JsonPrimitive(name)),
    )

    suspend fun setSubscriptionInterval(id: String, hours: Int): OperationResult = call(
        "set_subscription_interval",
        OperationResult.serializer(),
        args("id" to JsonPrimitive(id), "hours" to JsonPrimitive(hours)),
    )

    suspend fun removeSubscription(id: String): OperationResult =
        call("remove_subscription", OperationResult.serializer(), args("id" to JsonPrimitive(id)))

    // Plumbing.

    suspend fun <T> call(
        command: String,
        answer: KSerializer<T>,
        args: JsonObject = Envelope.empty,
    ): T = decode(answer, raw(command, args), command)

    /** The answer to [command] as JSON, for passing on to a `view.*` command unchanged. */
    suspend fun raw(command: String, args: JsonObject = Envelope.empty): JsonElement = withContext(dispatcher) {
        Envelope.read(transport.invoke(command, args.toString()))
    }

    private suspend fun control(cmd: String): JsonElement = controlRequest(buildJsonObject { put("cmd", cmd) })

    private suspend fun controlRequest(request: JsonObject): JsonElement =
        raw("runtime_control", buildJsonObject { put("request", request) })

    private fun <T> decode(answer: KSerializer<T>, element: JsonElement, command: String = "the client"): T = try {
        Envelope.json.decodeFromJsonElement(answer, element)
    } catch (error: IllegalArgumentException) {
        throw CoreException("Unexpected answer to $command: ${error.message}")
    }

    private inline fun <T> orNull(read: () -> T): T? = try {
        read()
    } catch (error: CoreException) {
        null
    }

    private fun args(vararg pairs: Pair<String, JsonElement>) = JsonObject(mapOf(*pairs))

    companion object {
        /** The delay test target every client uses. */
        const val DELAY_URL = "https://www.gstatic.com/generate_204"
    }
}

/** The proxies page: the view, and which of its nodes the user may edit and remove. */
data class ProxiesPage(
    val view: ProxiesView = ProxiesView(),
    val editable: Set<String> = emptySet(),
)

/** What the overview page reads in one go. */
data class OverviewData(
    val snapshot: RuntimeSnapshot,
    val overview: Overview,
    /** Open connections, oldest first; empty while not connected. */
    val connections: List<ConnectionSnapshot>,
)


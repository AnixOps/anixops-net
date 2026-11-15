package io.anixops.net.core

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** One node as `list_nodes` lists it (`bridge.ts` `NodeSummary`). */
@Serializable
data class NodeSummary(
    val id: String,
    val label: String,
    val protocol: String,
    val selected: Boolean = false,
    val group: String? = null,
    val editable: Boolean = false,
)

/** A node's edit form (`node_form`); [keepReason] says why it cannot be removed. */
@Serializable
data class NodeForm(
    val name: String,
    val server: String,
    val port: Int,
    val keepReason: String? = null,
)

/** One subscription as `list_subscriptions` lists it (`bridge.ts` `SubscriptionSummary`). */
@Serializable
data class SubscriptionSummary(
    val id: String,
    val location: String,
    val selected: Boolean = false,
    val name: String,
    val lastSuccessfulUpdate: String? = null,
    val lastAttempt: String? = null,
    val result: String = "",
    val lastUpdateError: String? = null,
    val nodeCount: Int? = null,
    val upload: Long? = null,
    val download: Long? = null,
    val total: Long? = null,
    /** Unix seconds. */
    val expire: Long? = null,
    /** 0 updates by hand only. */
    val updateIntervalHours: Int = 0,
    /** A link that can be downloaded again, not a file or pasted links. */
    val refreshable: Boolean = false,
) {
    /** Bytes used, when the server reports either direction. */
    val used: Long? get() = if (upload == null && download == null) null else (upload ?: 0) + (download ?: 0)
}

/** One node's result in a batch delay test (`test_node_delays`). */
@Serializable
data class NodeDelay(
    val nodeId: String,
    val delayMs: Long? = null,
    val error: String? = null,
)

/** One connection as the engine tracks it (`runtime_control` `connections`). */
@Serializable
data class ConnectionSnapshot(
    val id: Long,
    val network: String,
    val host: String? = null,
    val destination: String,
    val rule: String = "",
    val chain: List<String> = emptyList(),
    val outbound: String = "",
    @SerialName("started_at_ms") val startedAtMs: Long = 0,
    val upload: Long = 0,
    val download: Long = 0,
    val error: String? = null,
) {
    /** What the user recognises: the domain when the engine knows it, else the address. */
    val name: String get() = host?.takeIf { it.isNotBlank() } ?: destination

    /** Where it left: the last hop of the chain, else the outbound. */
    val via: String get() = chain.lastOrNull() ?: outbound
}

@Serializable
data class ConnectionsSnapshot(
    /** Oldest first. */
    val active: List<ConnectionSnapshot> = emptyList(),
)

/** The exit as seen from outside (`network_probe`). */
@Serializable
data class NetworkProbe(
    val exitIp: String? = null,
    /** Two-letter country code of the exit. */
    val exitLocation: String? = null,
)

/** The overview page (`view.overview`, crates/client-view). */
@Serializable
data class Overview(
    /** `connected`, `disconnected`, `failed` or `core_error`. */
    val state: String,
    /** `direct` or `node`. */
    val egress: String,
    val node: NodeSummary? = null,
    val canConnect: Boolean = false,
    val error: String? = null,
    val traffic: Traffic? = null,
)

/** Rates in bytes per second, totals in bytes, open connections. */
@Serializable
data class Traffic(
    val uploadRate: Long,
    val downloadRate: Long,
    val uploadTotal: Long,
    val downloadTotal: Long,
    val connections: Long,
)

/** The proxies page (`view.proxies`, crates/client-view): nodes searched and split by subscription. */
@Serializable
data class ProxiesView(
    val total: Int = 0,
    val shown: Int = 0,
    val sections: List<NodeSection> = emptyList(),
)

@Serializable
data class NodeSection(
    /** The subscription; null for nodes that belong to none. */
    val group: String? = null,
    val nodes: List<NodeRow>,
)

@Serializable
data class NodeRow(
    val id: String,
    val label: String,
    val protocol: String,
    val group: String? = null,
    val selected: Boolean = false,
    /** Null when never tested. */
    val delay: Delay? = null,
)

/** The last delay test: [ms] null when it got no answer; [tone] `fast`, `slow` or `failed`. */
@Serializable
data class Delay(
    val ms: Long? = null,
    val tone: String,
)

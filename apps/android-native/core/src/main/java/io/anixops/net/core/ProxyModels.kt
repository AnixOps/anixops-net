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

/** One subscription's row on the proxies page (`view.subscriptions`, crates/client-view). */
@Serializable
data class SubscriptionRow(
    val id: String,
    /** The core's name; for [pasted] links the window writes its own wording instead. */
    val name: String,
    val pasted: Boolean = false,
    val selected: Boolean = false,
    /** A link that can be downloaded again, not a file or pasted links. */
    val refreshable: Boolean = false,
    /** Null when the core has not counted them. */
    val nodes: Int? = null,
    /** When it was last downloaded, in the core's words. */
    val updated: String? = null,
    /** Why the last update did not work. */
    val error: String? = null,
    /** Bytes used, up and down together; null when the server did not say. */
    val used: Long? = null,
    /** Bytes allowed; null when the server did not say. */
    val total: Long? = null,
    /** Unix seconds; null when it does not expire or the server did not say. */
    val expire: Long? = null,
    val expired: Boolean = false,
    /** Hours between automatic updates, 0 by hand; read from `list_subscriptions`. */
    val intervalHours: Int = 0,
)

@Serializable
internal data class SubscriptionsView(val subscriptions: List<SubscriptionRow> = emptyList())

/** The part of a `list_subscriptions` entry the page needs beside `view.subscriptions`. */
@Serializable
internal data class SubscriptionInterval(val id: String, val updateIntervalHours: Int = 0)

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
    /** The subscription's name; null for nodes that belong to none. */
    val group: String? = null,
    /** The subscription's id. */
    val source: String? = null,
    /** Share links pasted into the client: the window writes its own heading. */
    val pasted: Boolean = false,
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

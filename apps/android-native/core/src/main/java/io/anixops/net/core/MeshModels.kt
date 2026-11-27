package io.anixops.net.core

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement

// The 组网 (mesh) answers of the client-view commands `view.mesh_*` (crates/client-view/src/mesh.rs,
// docs/architecture/mesh-client.md). The status snapshot itself stays JSON: it goes to those commands unchanged.

/** `view.mesh_entry`: the overview's 组网 section. */
@Serializable
data class MeshEntry(
    /** False while the engine has no mesh module: the entry is hidden. */
    val available: Boolean = false,
    val network: String? = null,
    /** `joined`, `joining`, `partial`, `not_joined` or `failed`. */
    val state: String = "not_joined",
    /** `forward_secrecy`, `standard` or `none`. */
    val security: String? = null,
    val online: Int = 0,
    val total: Int = 0,
    /** A notice is shown on the mesh page. */
    val attention: Boolean = false,
)

/** `view.mesh_members`: the member list, grouped (N28, N29). */
@Serializable
data class MeshMembers(
    /** `region`, `role` or `none`. */
    val groupBy: String = "none",
    val groups: List<MeshGroup> = emptyList(),
    val total: Int = 0,
    val matched: Int = 0,
)

@Serializable
data class MeshGroup(
    /** The region code or the role; null when not grouped. */
    val key: String? = null,
    val count: Int = 0,
    val matched: Int = 0,
    /** Shown unfolded: the local region, a group with a problem, or a search or filter. */
    val open: Boolean = true,
    val worst: MeshWorst? = null,
    /** The cross-region link to this region, for another region's summary. */
    val link: MeshGroupLink? = null,
    val rows: List<MeshMemberRow> = emptyList(),
)

@Serializable
data class MeshWorst(
    /** `unreachable` or `fallback`. */
    val state: String,
    val count: Int = 0,
)

@Serializable
data class MeshGroupLink(
    val latencyMs: Long? = null,
    val via: String? = null,
    val state: String = "ok",
)

@Serializable
data class MeshMemberRow(
    val id: String,
    val name: String = "",
    @SerialName("self") val isSelf: Boolean = false,
    val role: String? = null,
    val region: String? = null,
    val state: String = "online",
    val address: String? = null,
    val domain: String? = null,
    /** The region part of the internal domain name (`eu.mesh.internal`), the row's second line. */
    val regionDomain: String? = null,
    val connection: String = "unreachable",
    val hops: Int = 0,
    val transport: String? = null,
    val latencyMs: Long? = null,
    val tone: String? = null,
    val contains: MeshContains? = null,
    val children: List<MeshChild> = emptyList(),
    /** How many children show before 另 N 项 (N31). */
    val visibleChildren: Int = 0,
    val open: Boolean = false,
    /** `duplicate`, `unmanaged`, `fallback`, `first_hop_only`. */
    val marks: List<String> = emptyList(),
    /** Another member in this network's region has the same name (`duplicateOf`). */
    val duplicateInRegion: Boolean = false,
    /** Other networks that answer the same name (display names); only given when there are some. */
    val duplicateNetworks: List<String> = emptyList(),
)

@Serializable
data class MeshContains(
    /** `subnets` or `vms`. */
    val noun: String,
    val count: Int = 0,
)

@Serializable
data class MeshChild(
    /** `subnet`, `device` or `vm`. */
    val kind: String,
    val name: String = "",
    val address: String? = null,
    val domain: String? = null,
    /** The search matched this child. */
    val match: Boolean = false,
)

/** `view.mesh_subnets`. */
@Serializable
data class MeshSubnets(
    val rows: List<MeshSubnetRow> = emptyList(),
    /** Public networks refused, not listed. */
    val refused: Int = 0,
)

@Serializable
data class MeshSubnetRow(
    val cidr: String,
    val region: String? = null,
    val regionName: String? = null,
    /** The advertiser in use. */
    val advertiser: String? = null,
    val advertisers: List<MeshAdvertiser> = emptyList(),
    /** Member names, hop by hop. */
    val path: List<String> = emptyList(),
    val latencyMs: Long? = null,
    val tone: String? = null,
    /** `ok`, `unreachable` or `refused`. */
    val state: String = "ok",
    /** `local` or `mesh`. */
    val route: String = "mesh",
    val overlaps: List<String> = emptyList(),
    /** `multi_source`, `overlap`, `local_overlap`, `refused`. */
    val marks: List<String> = emptyList(),
)

@Serializable
data class MeshAdvertiser(val id: String, val name: String = "", val inUse: Boolean = false)

/** `view.mesh_paths`: default routing, cross-region links (kernel 0.4) and custom paths (kernel 0.5). */
@Serializable
data class MeshPaths(
    /** `latency` or `hops`. */
    val policy: String = "latency",
    /** Null before kernel 0.4: the section is not shown. */
    val links: List<MeshLink>? = null,
    /** Null before kernel 0.5: the section is not shown. */
    val custom: List<MeshCustomPath>? = null,
)

@Serializable
data class MeshLink(
    val from: String,
    val fromName: String = "",
    val to: String,
    val toName: String = "",
    val via: String? = null,
    val viaName: String? = null,
    val borders: List<String> = emptyList(),
    val latencyMs: Long? = null,
    val tone: String? = null,
    val loss: Double? = null,
    /** `ok`, `detour` or `down`. */
    val state: String = "ok",
)

@Serializable
data class MeshCustomPath(
    val id: String,
    /** `member` or `subnet`. */
    val targetKind: String,
    val target: String,
    val targetLabel: String = "",
    val advertiser: String? = null,
    val via: List<String> = emptyList(),
    /** `active`, `pending` (the path's nodes are being asked whether they forward on it), `fallback` or `first_hop_only`. */
    val state: String = "active",
    /** Why a path fell back: `unreachable`, `unsupported`, `not_relay`, `hop_limit`, `no_echo` or `pending_timeout` (mesh-status.md); on `fallback` and `first_hop_only`. */
    val reason: String? = null,
    val unreachable: String? = null,
)

/** One entry of `view.mesh_notices`, in the order of docs/mesh-ui.md 提示. */
@Serializable
data class MeshNotice(
    /** `mesh.notice.*`: the text's key. */
    val id: String,
    /** Stable while the same thing is wrong: a dismissed notice stays dismissed until it changes. */
    val key: String,
    /** The values the text names, in order (names, a percentage, a port). */
    val values: List<JsonElement> = emptyList(),
    /** `network_settings`, `take_over` or null. */
    val action: String? = null,
    /** Also sent once as a system notification. */
    val notify: Boolean = false,
)

/** The parts of the status snapshot the page shows as they are: the network and this device. */
@Serializable
data class MeshStatusHead(
    val network: MeshNetwork? = null,
    @SerialName("self") val own: MeshSelf? = null,
    val regions: List<MeshRegion> = emptyList(),
) {
    fun regionName(code: String?): String? = code?.let { wanted -> regions.firstOrNull { it.code == wanted }?.name ?: wanted }
}

@Serializable
data class MeshNetwork(
    val id: String = "",
    val name: String? = null,
    val state: String = "not_joined",
    val security: String? = null,
    val joining: MeshJoining? = null,
    val routingPolicy: String = "latency",
)

@Serializable
data class MeshJoining(
    /** `connecting`, `assigning_address` or `measuring_regions`. */
    val stage: String? = null,
    val peersConnected: Int? = null,
)

@Serializable
data class MeshSelf(
    val id: String = "",
    val name: String = "",
    val address: String? = null,
    val domain: String? = null,
    val region: String? = null,
    val regionBasis: MeshRegionBasis? = null,
    /** `auto`, `pinned` or `pending`. */
    val regionState: String = "auto",
)

@Serializable
data class MeshRegionBasis(val border: String? = null, val latencyMs: Long? = null)

@Serializable
data class MeshRegion(val code: String, val name: String = "", val local: Boolean = false)

/** What the mesh page reads in one go: the snapshot (for the commands that take it) and the answers. */
data class MeshPage(
    val status: JsonElement,
    val head: MeshStatusHead,
    val entry: MeshEntry,
    val members: MeshMembers,
    val subnets: MeshSubnets,
    val paths: MeshPaths,
    val notices: List<MeshNotice>,
)

/** Why a mesh command did not take effect: a failure code of docs/architecture/mesh-client.md, or `other`. */
data class MeshRefusal(val code: String, val detail: String? = null)

package io.anixops.net.core

import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.put

/**
 * Custom paths (docs/mesh-ui.md, 路径; kernel 0.5). The snapshot names members by id; `mesh_set_paths` takes the
 * kernel's `routing.paths`, which names them by virtual address, and the whole list each time. The same as the
 * desktop's `routingPaths` (mesh.ts) and the Apple `MeshPaths`.
 */
object MeshRouting {
    /** The most waypoints a path may name. */
    const val MAX_WAYPOINTS = 6

    /** A path as the editor holds it: a member id ([member]) or a subnet, and the waypoints by member id. */
    data class Draft(val member: Boolean, val target: String, val via: List<String>)

    /** The snapshot's paths: id to draft, in order. */
    fun current(status: JsonElement?): List<Pair<String, Draft>> =
        array(field(status, "paths")).map { path ->
            val target = field(path, "target")
            val via = array(field(path, "via")).mapNotNull { text(it) }
            val id = text(field(path, "id")).orEmpty()
            val peer = text(field(target, "peer"))
            if (peer != null) id to Draft(true, peer, via) else id to Draft(false, text(field(target, "subnet")).orEmpty(), via)
        }

    /**
     * `mesh_set_paths`' `paths` after removing the path [remove] (an id) and adding [add], which replaces a path to
     * the same target. Null when a member has no virtual address in the snapshot, or [add] names no waypoint or too
     * many.
     */
    fun paths(status: JsonElement?, remove: String? = null, add: Draft? = null): JsonArray? {
        val known = addresses(status)
        val drafts = current(status).filter { it.first != remove }.map { it.second }.toMutableList()
        if (add != null) {
            if (add.target.isEmpty() || add.via.isEmpty() || add.via.size > MAX_WAYPOINTS) return null
            drafts.removeAll { it.member == add.member && it.target == add.target }
            drafts += add
        }
        val entries = drafts.map { draft ->
            val via = draft.via.map { known[it] ?: return null }
            buildJsonObject {
                if (draft.member) put("to_peer", known[draft.target] ?: return null) else put("to_subnet", draft.target)
                put("via", JsonArray(via.map { JsonPrimitive(it) }))
            }
        }
        return JsonArray(entries)
    }

    /** Member id to virtual address without its prefix, this device included. */
    private fun addresses(status: JsonElement?): Map<String, String> {
        val rows = listOfNotNull(field(status, "self")) + array(field(status, "members"))
        return rows.mapNotNull { row ->
            val id = text(field(row, "id")) ?: return@mapNotNull null
            val address = text(field(row, "address"))?.substringBefore('/')?.takeIf { it.isNotEmpty() } ?: return@mapNotNull null
            id to address
        }.toMap()
    }

    private fun field(element: JsonElement?, key: String): JsonElement? = (element as? JsonObject)?.get(key)

    private fun array(element: JsonElement?): List<JsonElement> = (element as? JsonArray).orEmpty()

    private fun text(element: JsonElement?): String? = (element as? JsonPrimitive)?.takeIf { it.isString }?.contentOrNull
}

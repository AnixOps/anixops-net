package io.anixops.net.core

import java.io.File
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.longOrNull
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** Every `view.*` answer the app reads in the client-view samples decodes into its models. */
class ViewModelsTest {
    private val cases: List<JsonObject> by lazy {
        val path = System.getProperty("viewCases") ?: error("the viewCases system property is not set")
        Envelope.json.parseToJsonElement(File(path).readText()).jsonArray.map { it.jsonObject }
    }

    private fun answers(command: String) =
        cases.filter { it["command"]?.jsonPrimitive?.content == command && "answer" in it }.map { it.getValue("answer") }

    @Test
    fun overviewAnswersDecode() {
        val answers = answers("view.overview")
        assertTrue("no view.overview samples", answers.isNotEmpty())
        val overviews = answers.map { Envelope.json.decodeFromJsonElement(Overview.serializer(), it) }
        assertTrue(overviews.all { it.state in setOf("connected", "disconnected", "failed", "core_error") })
        assertTrue(overviews.all { it.egress in setOf("node", "direct") })
        assertTrue(overviews.any { it.traffic != null })
    }

    @Test
    fun proxiesAnswersDecode() {
        val answers = answers("view.proxies")
        assertTrue("no view.proxies samples", answers.isNotEmpty())
        answers.forEach { answer ->
            val view = Envelope.json.decodeFromJsonElement(ProxiesView.serializer(), answer)
            assertEquals(view.shown, view.sections.sumOf { it.nodes.size })
            view.sections.flatMap { it.nodes }.mapNotNull { it.delay }.forEach { delay ->
                assertTrue(delay.tone in setOf("fast", "slow", "failed"))
            }
        }
    }

    @Test
    fun subscriptionsAnswersDecode() {
        val answers = answers("view.subscriptions")
        assertTrue("no view.subscriptions samples", answers.isNotEmpty())
        val rows = answers.flatMap { Envelope.json.decodeFromJsonElement(SubscriptionsView.serializer(), it).subscriptions }
        assertTrue(rows.all { it.id.isNotEmpty() })
        assertTrue(rows.filter { it.expired }.all { it.expire != null })
    }

    @Test
    fun logsAnswersDecode() {
        val answers = answers("view.logs")
        assertTrue("no view.logs samples", answers.isNotEmpty())
        answers.forEach { answer ->
            val view = Envelope.json.decodeFromJsonElement(LogsView.serializer(), answer)
            assertTrue(view.shown.all { it.module in view.modules })
            assertEquals(view.shown.size, if (view.text.isEmpty()) 0 else view.text.lines().size)
        }
    }

    @Test
    fun trafficStatsAnswersDecode() {
        val answers = answers("view.traffic_stats")
        assertTrue("no view.traffic_stats samples", answers.isNotEmpty())
        answers.forEach { answer ->
            val view = Envelope.json.decodeFromJsonElement(TrafficStatsView.serializer(), answer)
            assertTrue(view.bars.size == 24 || view.bars.size == 7)
            assertEquals(view.upload + view.download, view.bars.sumOf { it.bytes })
        }
    }

    private fun samples(command: String) = cases.filter { it["command"]?.jsonPrimitive?.content == command && "answer" in it }

    /** The list's cursor is worked out in Kotlin; it gives what client-view's `next_since` gives for its samples. */
    @Test
    fun nextSinceFollowsTheClientViewSamples() {
        val samples = samples("view.next_since")
        assertTrue("no view.next_since samples", samples.isNotEmpty())
        samples.forEach { case ->
            val args = case.getValue("args").jsonObject
            val rows = args["rows"]?.jsonArray ?: JsonArray(emptyList())
            val lastSeq = args["lastSeq"]?.jsonPrimitive?.longOrNull ?: 0L
            assertEquals(case.toString(), case.getValue("answer").jsonPrimitive.long, nextSince(rows, lastSeq))
        }
    }

    @Test
    fun nextRevFollowsTheClientViewSamples() {
        val samples = samples("view.next_rev")
        assertTrue("no view.next_rev samples", samples.isNotEmpty())
        samples.forEach { case ->
            val args = case.getValue("args").jsonObject
            val current = (args["current"] as? JsonPrimitive)?.longOrNull
            val page = args["page"]?.jsonPrimitive?.intOrNull ?: 0
            val expected = (case.getValue("answer") as? JsonPrimitive)?.longOrNull
            assertEquals(case.toString(), expected, nextRev(current, args["answer"] ?: JsonNull, page))
        }
    }

    /**
     * Merging an answer with no request and no row pushed out gives rows from an earlier merge (already in order)
     * back as they are, so the list skips the merge.
     */
    @Test
    fun anEmptyAnswerChangesNothing() {
        val rows = Envelope.json.parseToJsonElement("""[{"seq":3,"connection_id":1,"index":3},{"seq":0,"connection_id":1,"index":4}]""").jsonArray
        assertTrue(mergeChangesNothing(rows, Envelope.json.parseToJsonElement("""{"requests":[],"last_seq":3}""")))
        assertTrue(mergeChangesNothing(rows, Envelope.json.parseToJsonElement("""{"last_seq":3,"dropped_before_seq":2}""")))
        assertFalse(mergeChangesNothing(rows, Envelope.json.parseToJsonElement("""{"requests":[],"dropped_before_seq":3}""")))
        assertFalse(mergeChangesNothing(rows, Envelope.json.parseToJsonElement("""{"requests":[{"seq":4,"connection_id":1,"index":4}]}""")))
        assertFalse(mergeChangesNothing(rows, JsonNull))
    }

    /** Every `view.mesh_*` answer of the samples decodes into the mesh models, and the status heads decode too. */
    @Test
    fun meshAnswersDecode() {
        val entries = answers("view.mesh_entry").map { Envelope.json.decodeFromJsonElement(MeshEntry.serializer(), it) }
        assertTrue("no view.mesh_entry samples", entries.isNotEmpty())
        assertTrue(entries.any { !it.available })
        assertTrue(entries.any { it.available && it.network != null })
        val members = answers("view.mesh_members").map { Envelope.json.decodeFromJsonElement(MeshMembers.serializer(), it) }
        assertTrue("no view.mesh_members samples", members.isNotEmpty())
        members.forEach { view ->
            assertEquals(view.matched, view.groups.sumOf { it.rows.size })
            view.groups.flatMap { it.rows }.forEach { row -> assertTrue(row.visibleChildren <= row.children.size) }
        }
        assertTrue(members.flatMap { it.groups }.flatMap { it.rows }.any { it.isSelf })
        val subnets = answers("view.mesh_subnets").map { Envelope.json.decodeFromJsonElement(MeshSubnets.serializer(), it) }
        assertTrue("no view.mesh_subnets samples", subnets.isNotEmpty())
        assertTrue(subnets.flatMap { it.rows }.all { it.state in setOf("ok", "unreachable", "refused") })
        val paths = answers("view.mesh_paths").map { Envelope.json.decodeFromJsonElement(MeshPaths.serializer(), it) }
        assertTrue("no view.mesh_paths samples", paths.isNotEmpty())
        assertTrue(paths.any { it.links != null } && paths.any { it.custom != null })
        val notices = answers("view.mesh_notices").flatMap {
            Envelope.json.decodeFromJsonElement(ListSerializer(MeshNotice.serializer()), it)
        }
        assertTrue(notices.all { it.id.startsWith("mesh.notice.") })
        val heads = cases.filter { it["command"]?.jsonPrimitive?.content?.startsWith("view.mesh_") == true }
            .mapNotNull { it["args"]?.jsonObject?.get("status") as? JsonObject }
            .map { Envelope.json.decodeFromJsonElement(MeshStatusHead.serializer(), it) }
        assertTrue(heads.any { it.network != null && it.own != null && it.regions.isNotEmpty() })
    }

    @Test
    fun meshDuplicateScopesDecodeIndependently() {
        val old = Envelope.json.decodeFromString(MeshMemberRow.serializer(), """{"id":"a","name":"host","marks":["duplicate"]}""")
        assertTrue(old.marks.contains("duplicate"))
        assertFalse(old.duplicateInNetwork)
        assertFalse(old.duplicateAcrossRegions)
        val current = Envelope.json.decodeFromString(
            MeshMemberRow.serializer(),
            """{"id":"a","name":"host","duplicateInNetwork":true,"duplicateAcrossRegions":true,"duplicateNetworks":["Lab"]}""",
        )
        assertFalse(current.duplicateInRegion)
        assertTrue(current.duplicateInNetwork)
        assertTrue(current.duplicateAcrossRegions)
        assertEquals(listOf("Lab"), current.duplicateNetworks)
    }
}

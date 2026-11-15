package io.anixops.net.core

import java.io.File
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/** Every `view.overview` and `view.proxies` answer in the client-view samples decodes into the app's models. */
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
}

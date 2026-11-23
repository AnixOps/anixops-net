package io.anixops.net

import io.anixops.net.core.CoreTexts
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNotEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * Every result and refusal code the shared client sends has words in English, Simplified and Traditional Chinese,
 * so no screen shows the client's English (finding ux-i18n-a11y-02).
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class CodeTextsTest {
    private fun texts() = CodeTexts(RuntimeEnvironment.getApplication())

    /** The codes in crates/client-core's sources: results (`<area>.<name>`) and refusals (`error.<name>`). */
    private fun clientCodes(): Set<String> {
        val pattern = Regex("\"((?:${AREAS.joinToString("|")})\\.[a-z_]+)\"")
        return File(CLIENT_CORE).walk()
            .filter { it.isFile && it.extension == "rs" }
            .flatMap { file -> pattern.findAll(file.readText()).map { it.groupValues[1] } }
            .filterNot { Regex("""\.(json|log|txt|rs|ya?ml)$""").containsMatchIn(it) }
            .toSet()
    }

    @Test
    fun everyCodeTheClientSendsHasWords() {
        val codes = clientCodes()
        assertTrue("read ${File(CLIENT_CORE).absolutePath}", codes.size > 80)
        assertEquals(emptyList<String>(), codes.filterNot { it in CodeTexts.codes }.sorted())
    }

    @Test
    fun theWordsAreInEachLanguage() {
        for (qualifiers in listOf("en", "zh-rCN", "zh-rTW")) {
            RuntimeEnvironment.setQualifiers(qualifiers)
            val texts = texts()
            for (code in CodeTexts.codes.keys) {
                val text = texts.text(code, emptyMap())
                assertTrue("$qualifiers $code", !text.isNullOrBlank() && text != code)
            }
        }
        RuntimeEnvironment.setQualifiers("en")
        val english = texts().text("routing.set_rule", emptyMap())
        RuntimeEnvironment.setQualifiers("zh-rCN")
        val simplified = texts().text("routing.set_rule", emptyMap())
        RuntimeEnvironment.setQualifiers("zh-rTW")
        val traditional = texts().text("routing.set_rule", emptyMap())
        assertNotEquals(english, simplified)
        assertNotEquals(simplified, traditional)
    }

    @Test
    @Config(qualifiers = "zh-rCN")
    fun resultsFillTheirParametersAndRefusalsKeepTheEnglishOut() {
        val texts = texts()
        assertEquals("已通过 东京 01 连接", texts.text("connect.connected_via", mapOf("node" to "东京 01")))
        assertEquals("请输入名称", texts.text("error.name_required", emptyMap()))
        assertEquals("操作没有完成：disk full", texts.failed("disk full"))
        // The kernel's subscription refusals, by how their kind ends.
        assertEquals("订阅里有本应用不支持的节点类型或写法", texts.text("subscription.core.clash_yaml_unsupported", emptyMap()))
        assertEquals("订阅里有写法不正确的节点", texts.text("subscription.core.vmess_link_invalid", emptyMap()))
        assertNull(texts.text("future.thing", emptyMap()))
        assertEquals("将解密 a.example。下次连接时生效", texts.join("将解密 a.example", texts.text("plugins.applied_next", emptyMap())!!))
    }

    @Test
    @Config(qualifiers = "en")
    fun aRoutingWarningNamesItsKind() {
        val texts = texts()
        assertEquals(
            "Policy group skipped" to "policy group Chain was skipped: type relay is unsupported",
            texts.routingWarning(
                "config.core.routing_import.group_skipped: policy group Chain was skipped: type relay is unsupported",
            ),
        )
        assertEquals("4 IP-ASN rules were not imported" to null, texts.routingWarning("4 IP-ASN rules were not imported"))
        assertEquals("error.name_required" to "enter a name", CoreTexts.split("error.name_required: enter a name"))
    }

    private companion object {
        const val CLIENT_CORE = "../../../crates/client-core/src"

        /** The areas codes are named in, as the desktop window's operation-text.test.ts reads them, and `error`. */
        val AREAS = listOf(
            "certificate", "connect", "crash_reports", "data_lists", "decrypt", "delay", "devices", "diagnostics",
            "disconnect", "dns", "driver", "egress", "fastest", "group", "links", "mapping", "mitm", "node", "nodes",
            "plugins", "preferences", "profile", "proxy", "quic", "redirects", "restart", "root", "routing", "rules",
            "script_runtime", "subscription", "tun", "validate", "error",
        )
    }
}

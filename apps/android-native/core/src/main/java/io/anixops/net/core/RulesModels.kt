package io.anixops.net.core

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** The user's own rules (`my_rules`): the text as written and how many rules it holds. */
@Serializable
data class MyRules(val text: String = "", val count: Int = 0)

/** A policy group of the profile (`list_native_groups`). */
@Serializable
data class NativeGroup(
    val tag: String,
    /** `selector` (picked by hand), `urltest` (fastest) or another kind. */
    val groupType: String,
    val selected: String? = null,
    val outbounds: List<String> = emptyList(),
    /** Display names for [outbounds]; empty when they are the names. */
    val outboundLabels: List<String> = emptyList(),
) {
    fun label(outbound: String?): String? {
        if (outbound == null) return null
        val index = outbounds.indexOf(outbound)
        return outboundLabels.getOrNull(index)?.takeIf { it.isNotBlank() } ?: outbound
    }
}

/** A rule set of the running engine (`runtime_control` `rule_sets`). */
@Serializable
data class RuleSetStatus(
    val name: String,
    /** `inline` or `remote`. */
    val source: String,
    val entries: Int = 0,
    @SerialName("updated_at_ms") val updatedAtMs: Long? = null,
    @SerialName("last_error") val lastError: String? = null,
)

@Serializable
internal data class RuleSetsAnswer(@SerialName("rule_sets") val ruleSets: List<RuleSetStatus> = emptyList())

@Serializable
internal data class ScheduledAnswer(val scheduled: Int = 0)

/** The 自定义 DNS text and the Fake-IP switch (`custom_dns`, `save_custom_dns`). */
@Serializable
data class DnsText(
    val servers: String = "",
    val rules: String = "",
    val hosts: String = "",
    @SerialName("fake_ip") val fakeIp: Boolean = false,
)

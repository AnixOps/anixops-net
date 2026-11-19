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
    /**
     * Lines of the last download the set could not hold, by rule type (`SCRIPT`, `INVALID` for malformed
     * lines, `OTHER` past sixteen types); empty when none were.
     */
    val skipped: Map<String, Int> = emptyMap(),
) {
    /** How many lines [skipped] counts in all. */
    val skippedCount: Int get() = skipped.values.sum()

    /** [skipped] as `SCRIPT ×12, OTHER ×1`, most first. */
    val skippedSummary: String
        get() = skipped.entries
            .sortedWith(compareByDescending<Map.Entry<String, Int>> { it.value }.thenBy { it.key })
            .joinToString(", ") { "${it.key} ×${it.value}" }
}

/**
 * The running engine's rule sets and the data lists its GEOIP and IP-ASN rules use that nobody provides
 * (`geoip-us`, `asn-13335`): those rules match nothing until the user adds a list.
 */
@Serializable
data class RuleSetsView(
    @SerialName("rule_sets") val ruleSets: List<RuleSetStatus> = emptyList(),
    val missing: List<String> = emptyList(),
)

/** A GEOIP or IP-ASN data list the user added by URL (`data_sources`). */
@Serializable
data class DataSource(val name: String, val url: String)

/** One rule and how often it decided a connection since the rules were loaded (`rule_hits`). */
@Serializable
data class RuleHit(
    val rule: String,
    val target: String,
    val hits: Long = 0,
    @SerialName("last_hit_at_ms") val lastHitAtMs: Long? = null,
)

@Serializable
data class RuleHitsView(
    val generation: Long = 0,
    val rules: List<RuleHit> = emptyList(),
    @SerialName("final") val finalRule: RuleHit,
) {
    /** The rules that decided at least one connection, most first, at most [limit]. */
    fun busiest(limit: Int = 50): List<RuleHit> = rules.filter { it.hits > 0 }.sortedByDescending { it.hits }.take(limit)

    /** How many rules decided nothing yet. */
    val unused: Int get() = rules.count { it.hits == 0L }
}

/** Which rule decides a made-up connection and where it goes (`test_route`). */
@Serializable
data class RouteTest(
    /** Index in [RuleHitsView.rules]; null for FINAL. */
    @SerialName("rule_index") val ruleIndex: Int? = null,
    val rule: String,
    val chain: List<String> = emptyList(),
    val outbound: String,
    /** The rule needs the destination's IP address, which the test was not given; a failed lookup rejects. */
    @SerialName("needs_address") val needsAddress: Boolean = false,
) {
    /** The groups and the outbound, each once: `节点选择 → 香港 01`. */
    val path: String get() = (chain + outbound).distinct().joinToString(" → ")
}

/** [RouteTest] with the routing mode (`rule`, `global` or `direct`); outside `rule` the rules do not apply. */
@Serializable
data class RouteTestAnswer(val mode: String = "rule", val test: RouteTest)

/** The rule a data list serves, as the user writes it: `geoip-us` is `GEOIP,US`, `asn-13335` is `IP-ASN,13335`. */
fun dataRule(name: String): String = when {
    name.startsWith("geoip-") -> "GEOIP," + name.removePrefix("geoip-").uppercase()
    name.startsWith("asn-") -> "IP-ASN," + name.removePrefix("asn-")
    else -> name
}

/** The list name for what the user typed: `us`, `GEOIP,US` or `geoip-us`; `AS13335`, `IP-ASN,13335` or `asn-13335`. */
fun dataName(text: String): String? {
    val value = text.trim().lowercase()
        .replace(Regex("^geoip[,-]\\s*"), "geoip-")
        .replace(Regex("^ip-asn,\\s*"), "asn-")
        .replace(Regex("^(asn-)?as(?=\\d)"), "asn-")
    if (Regex("^[a-z]{2}$").matches(value)) return "geoip-$value"
    if (Regex("^geoip-[a-z]{2}$").matches(value)) return value
    val number = Regex("^(?:asn-)?([1-9]\\d{0,9})$").find(value)?.groupValues?.get(1) ?: return null
    return if (number.toLong() <= 4_294_967_295L) "asn-$number" else null
}

/** Whether [text] (the user's own rules) has a PROCESS-NAME line, which the Android VPN cannot match. */
fun hasProcessRules(text: String): Boolean =
    text.lineSequence().any { it.trim().startsWith("PROCESS-NAME", ignoreCase = true) }

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

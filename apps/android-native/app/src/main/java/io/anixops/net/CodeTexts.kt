package io.anixops.net

import android.content.Context
import androidx.annotation.StringRes
import io.anixops.net.core.CoreTexts

/**
 * The client's result and refusal codes in the app's language (finding ux-i18n-a11y-02): the same words as the
 * desktop window (`apps/windows-gui/ui/src/operation-text.ts`, `error-text.ts`), from the string resources, so they
 * follow the app's language. Read through [context] on every call, so a language change applies at once.
 * CodeTextsTest checks that every code `crates/client-core` sends is here in all three languages.
 */
class CodeTexts(private val context: Context) : CoreTexts {
    override fun text(code: String, params: Map<String, String>): String? =
        resource(code)?.let { CoreTexts.fill(context.getString(it), params) }

    override fun join(first: String, then: String): String =
        CoreTexts.fill(context.getString(R.string.code_follow_join), mapOf("first" to first, "then" to then))

    override fun failed(detail: String): String =
        CoreTexts.fill(context.getString(R.string.code_failed), mapOf("detail" to detail))

    override fun unreadable(): String = context.getString(R.string.code_unreadable)

    /**
     * A line of `routing_warnings` (`<kernel code>: <English>`): what kind of line it is in the app's language and
     * the kernel's English, which names the rule or group; an older client's plain line is the text alone.
     */
    fun routingWarning(line: String): Pair<String, String?> {
        val (code, english) = CoreTexts.split(line) ?: return line to null
        val kind = codes[code] ?: return line to null
        return context.getString(kind) to english
    }

    companion object {
        /** Each code's string. */
        val codes: Map<String, Int> = mapOf(
            "connect.connected_via" to R.string.code_connect_connected_via,
            "connect.connected_via_at" to R.string.code_connect_connected_via_at,
            "crash_reports.off" to R.string.code_crash_reports_off,
            "crash_reports.on" to R.string.code_crash_reports_on,
            "data_lists.downloading" to R.string.code_data_lists_downloading,
            "data_lists.next" to R.string.code_data_lists_next,
            "decrypt.decrypting" to R.string.code_decrypt_decrypting,
            "decrypt.excluding" to R.string.code_decrypt_excluding,
            "decrypt.on" to R.string.code_decrypt_on,
            "decrypt.paused" to R.string.code_decrypt_paused,
            "decrypt.removed" to R.string.code_decrypt_removed,
            "decrypt.saved" to R.string.code_decrypt_saved,
            "delay.node" to R.string.code_delay_node,
            "devices.next" to R.string.code_devices_next,
            "devices.restarted" to R.string.code_devices_restarted,
            "diagnostics.saved" to R.string.code_diagnostics_saved,
            "disconnect.done" to R.string.code_disconnect_done,
            "dns.set_custom" to R.string.code_dns_set_custom,
            "dns.set_encrypted" to R.string.code_dns_set_encrypted,
            "dns.set_system" to R.string.code_dns_set_system,
            "dns.rule_sets_missing" to R.string.code_dns_rule_sets_missing,
            "dns.settings_saved_custom" to R.string.code_dns_settings_saved_custom,
            "dns.settings_saved_next" to R.string.code_dns_settings_saved_next,
            "dns.settings_saved_restarted" to R.string.code_dns_settings_saved_restarted,
            "egress.direct" to R.string.code_egress_direct,
            "egress.node" to R.string.code_egress_node,
            "egress.unchanged" to R.string.code_egress_unchanged,
            "fastest.selected_delay" to R.string.code_fastest_selected_delay,
            "group.member_reconnected" to R.string.code_group_member_reconnected,
            "group.member_set" to R.string.code_group_member_set,
            "links.imported_before" to R.string.code_links_imported_before,
            "mapping.removed" to R.string.code_mapping_removed,
            "mapping.saved" to R.string.code_mapping_saved,
            "mitm.off" to R.string.code_mitm_off,
            "mitm.on" to R.string.code_mitm_on,
            "mitm.on_next" to R.string.code_mitm_on_next,
            "mitm.root_untrusted" to R.string.code_mitm_root_untrusted,
            "node.edit_saved" to R.string.code_node_edit_saved,
            "node.edit_saved_restarted" to R.string.code_node_edit_saved_restarted,
            "node.removed" to R.string.code_node_removed,
            "node.removed_restarted" to R.string.code_node_removed_restarted,
            "node.switched_to" to R.string.code_node_switched_to,
            "nodes.imported" to R.string.code_nodes_imported,
            "plugins.applied_next" to R.string.code_plugins_applied_next,
            "plugins.applied_running" to R.string.code_plugins_applied_running,
            "plugins.disabled" to R.string.code_plugins_disabled,
            "plugins.enabled" to R.string.code_plugins_enabled,
            "plugins.imported" to R.string.code_plugins_imported,
            "plugins.imported_off" to R.string.code_plugins_imported_off,
            "plugins.need_decryption" to R.string.code_plugins_need_decryption,
            "plugins.removed" to R.string.code_plugins_removed,
            "preferences.saved" to R.string.code_preferences_saved,
            "proxy.not_managed" to R.string.code_proxy_not_managed,
            "proxy.restored" to R.string.code_proxy_restored,
            "quic.allowed" to R.string.code_quic_allowed,
            "quic.blocked" to R.string.code_quic_blocked,
            "redirects.applied" to R.string.code_redirects_applied,
            "redirects.next" to R.string.code_redirects_next,
            "root.in_use" to R.string.code_root_in_use,
            "root.removed_browsers" to R.string.code_root_removed_browsers,
            "root.removed_browsers_system" to R.string.code_root_removed_browsers_system,
            "root.removed_nowhere" to R.string.code_root_removed_nowhere,
            "root.removed_system" to R.string.code_root_removed_system,
            "root.removed_user" to R.string.code_root_removed_user,
            "root.saved_next" to R.string.code_root_saved_next,
            "root.trusted_browsers" to R.string.code_root_trusted_browsers,
            "root.trusted_browsers_system" to R.string.code_root_trusted_browsers_system,
            "root.trusted_nowhere" to R.string.code_root_trusted_nowhere,
            "root.trusted_system" to R.string.code_root_trusted_system,
            "root.trusted_user" to R.string.code_root_trusted_user,
            "routing.set_direct" to R.string.code_routing_set_direct,
            "routing.set_global" to R.string.code_routing_set_global,
            "routing.set_rule" to R.string.code_routing_set_rule,
            "rules.applied" to R.string.code_rules_applied,
            "rules.saved_next" to R.string.code_rules_saved_next,
            "rules.saved_restarted" to R.string.code_rules_saved_restarted,
            "subscription.interval" to R.string.code_subscription_interval,
            "subscription.interval_off" to R.string.code_subscription_interval_off,
            "subscription.removed" to R.string.code_subscription_removed,
            "subscription.renamed" to R.string.code_subscription_renamed,
            "subscription.selected_reconnect" to R.string.code_subscription_selected_reconnect,
            "subscription.switched_groups_choose" to R.string.code_subscription_switched_groups_choose,
            "subscription.updated_count" to R.string.code_subscription_updated_count,
            "tun.off_next" to R.string.code_tun_off_next,
            "tun.on_next" to R.string.code_tun_on_next,
            "tun.on_next_password" to R.string.code_tun_on_next_password,
            "validate.supported" to R.string.code_validate_supported,
            "error.name_required" to R.string.code_error_name_required,
            "error.subscription_missing" to R.string.code_error_subscription_missing,
            "error.subscription_removed" to R.string.code_error_subscription_removed,
            "error.no_supported_nodes" to R.string.code_error_no_supported_nodes,
            "error.no_nodes" to R.string.code_error_no_nodes,
            "error.node_missing" to R.string.code_error_node_missing,
            "error.node_not_editable" to R.string.code_error_node_not_editable,
            "error.no_subscription" to R.string.code_error_no_subscription,
            "error.no_policy_groups" to R.string.code_error_no_policy_groups,
            "error.import_input_required" to R.string.code_error_import_input_required,
            "error.disconnect_first" to R.string.code_error_disconnect_first,
            "error.connect_first_replay" to R.string.code_error_connect_first_replay,
            "error.no_delay_answer" to R.string.code_error_no_delay_answer,
            "error.not_updatable" to R.string.code_error_not_updatable,
            "error.no_default_route" to R.string.code_error_no_default_route,
            "error.vpn_off" to R.string.code_error_vpn_off,
            "error.rules_rejected" to R.string.code_error_rules_rejected,
            "error.mesh_gated" to R.string.code_error_mesh_gated,
            "config.core.routing_import.rule_skipped" to R.string.code_config_core_routing_import_rule_skipped,
            "config.core.routing_import.group_skipped" to R.string.code_config_core_routing_import_group_skipped,
            "config.core.routing_import.member_skipped" to R.string.code_config_core_routing_import_member_skipped,
            "config.core.routing_import.option_ignored" to R.string.code_config_core_routing_import_option_ignored,
            "config.core.routing_import.invalid" to R.string.code_config_core_routing_import_invalid,
            "routing_warnings.more" to R.string.code_routing_warnings_more,
        )

        /**
         * The resource for [code]: its own, or for the kernel's subscription refusals (`subscription.core.<kind>`)
         * one by how the kind ends, as the desktop window words them (`subscription-format.ts`).
         */
        @StringRes
        fun resource(code: String): Int? {
            codes[code]?.let { return it }
            val kind = code.removePrefix("subscription.core.").takeIf { it != code } ?: return null
            return when {
                kind.endsWith("_unsupported") -> R.string.code_subscription_core_unsupported
                kind.endsWith("_invalid") -> R.string.code_subscription_core_invalid
                kind == "parse_failed" -> R.string.code_subscription_core_parse_failed
                kind == "inline_payload_empty" || kind == "location_empty" -> R.string.code_subscription_core_empty
                else -> null
            }
        }
    }
}

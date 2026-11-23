package io.anixops.net.ui

import androidx.annotation.StringRes
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import io.anixops.net.CodeTexts
import io.anixops.net.R
import io.anixops.net.core.DnsText
import io.anixops.net.core.MyRules
import io.anixops.net.core.NativeGroup
import io.anixops.net.core.RuleSetStatus
import io.anixops.net.core.RuntimeSnapshot
import io.anixops.net.core.hasProcessRules
import io.anixops.net.core.hasRuleKind
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette

/** What the rules page can ask for. */
data class RulesActions(
    val setRoutingMode: (String) -> Unit = {},
    val saveMyRules: (String) -> Unit = {},
    val selectGroupOutbound: (group: String, outbound: String) -> Unit = { _, _ -> },
    val updateRuleSets: () -> Unit = {},
    val setDnsPreset: (String) -> Unit = {},
    val saveDns: (DnsText) -> Unit = {},
    val setBlockQuic: (Boolean) -> Unit = {},
    val saveDataSource: (name: String, url: String) -> Unit = { _, _ -> },
    val removeDataSource: (String) -> Unit = {},
    val clearRuleHits: () -> Unit = {},
    val testRoute: (host: String, port: Int, network: String, address: String, process: String) -> Unit = { _, _, _, _, _ -> },
)

/** A choice of a one-of list: its value, name and the sentence under the name. */
private data class Choice(val value: String, @param:StringRes val label: Int, @param:StringRes val hint: Int)

private val DnsChoices = listOf(
    Choice("system", R.string.dns_system, R.string.dns_system_hint),
    Choice("encrypted", R.string.dns_encrypted, R.string.dns_encrypted_hint),
    Choice("custom", R.string.dns_custom, R.string.dns_custom_hint),
)

/** Examples shown in the empty editors; in common rule syntax (DnsCards.tsx, MyRules.tsx). */
private const val RulesPlaceholder =
    "DOMAIN-SUFFIX,example.com,PROXY\nDOMAIN-KEYWORD,ads,REJECT\nIP-CIDR,10.0.0.0/8,DIRECT,no-resolve"
private const val ServersPlaceholder =
    "cloudflare = https://1.1.1.1/dns-query, server-name=cloudflare-dns.com\n" +
        "alidns = quic://223.5.5.5, server-name=dns.alidns.com, direct, default\n" +
        "local = system, fallback"
private const val DnsRulesPlaceholder = "DOMAIN-SUFFIX,example.cn,alidns\nDOMAIN-KEYWORD,intranet,alidns"
private const val HostsPlaceholder = "router.lan = 192.168.1.1\n*.home.lan = 192.168.1.20, fd00::20"

/**
 * 规则: the routing mode, the user's own rules, policy groups, the running rule sets, DNS with Fake-IP, and
 * 屏蔽 QUIC, as the other clients' rules pages. The engine runs in the app, so a DNS change restarts it at once.
 */
@Composable
fun RulesScreen(
    snapshot: RuntimeSnapshot?,
    appBusy: Boolean,
    state: RulesState,
    actions: RulesActions,
    modifier: Modifier = Modifier,
    onBack: (() -> Unit)? = null,
) {
    val enabled = snapshot != null && !appBusy && !state.busy
    Page(stringResource(R.string.nav_rules), modifier, onBack) {
        RoutingSection(snapshot?.routingMode, enabled, actions)
        state.myRules?.let { MyRulesSection(it, enabled, actions.saveMyRules) }
        GroupsSection(state.groups, enabled, actions.selectGroupOutbound)
        // The engine answers rule hits only while it runs; the test needs it too.
        state.hits?.let {
            RuleTestSection(state.routeTest, enabled, actions.testRoute)
            RuleHitsSection(it, enabled, actions.clearRuleHits)
        }
        DataListsSection(state.missingData, state.dataSources, enabled, actions.saveDataSource, actions.removeDataSource)
        if (state.ruleSets.isNotEmpty()) RuleSetsSection(state.ruleSets, enabled, actions.updateRuleSets)
        if (state.routingWarnings.isNotEmpty()) RoutingWarningsSection(state.routingWarnings)
        DnsSection(snapshot, state.dns, enabled, actions)
        val dns = state.dns
        if (snapshot?.dnsPreset == "custom" && dns != null) CustomDnsSection(dns, enabled, actions.saveDns)
        Section(title = null) {
            SwitchRow(
                stringResource(R.string.block_quic),
                stringResource(R.string.block_quic_hint),
                checked = snapshot?.blockQuic == true,
                enabled = enabled,
                onChange = actions.setBlockQuic,
            )
        }
    }
}

@Composable
private fun RoutingSection(current: String?, enabled: Boolean, actions: RulesActions) {
    Column {
        Section(stringResource(R.string.home_routing)) {
            Column(Modifier.selectableGroup()) {
                RoutingModes.forEachIndexed { index, mode ->
                    if (index > 0) RowDivider()
                    ChoiceRow(
                        routingLabel(mode),
                        routingHint(mode),
                        selected = mode == current,
                        enabled = enabled,
                        onClick = { if (mode != current) actions.setRoutingMode(mode) },
                    )
                }
            }
        }
        Note(stringResource(R.string.rules_routing_note))
    }
}

@Composable
private fun routingHint(mode: String): String = stringResource(
    when (mode) {
        "direct" -> R.string.routing_direct_hint
        else -> R.string.routing_rule_hint
    },
)

@Composable
private fun MyRulesSection(saved: MyRules, enabled: Boolean, onSave: (String) -> Unit) {
    var draft by rememberSaveable(saved.text) { mutableStateOf(saved.text) }
    val changed = draft != saved.text
    val title = stringResource(R.string.rules_mine)
    Column {
        Section(
            title,
            action = {
                Text(
                    pluralStringResource(R.plurals.rules_count, saved.count, saved.count),
                    style = MaterialTheme.typography.labelMedium,
                    color = LocalBrandPalette.current.text2,
                    modifier = Modifier.padding(end = BrandSpacing.md),
                )
            },
        ) {
            Editor(draft, { draft = it }, title, RulesPlaceholder, Modifier.padding(BrandSpacing.md))
            SaveRow(changed, enabled, onRevert = { draft = saved.text }, onSave = { onSave(draft) })
        }
        if (hasProcessRules(saved.text)) ProcessRulesWarning()
        if (hasRuleKind(saved.text, "DEVICE-NAME")) RuleWarning(stringResource(R.string.rules_device_not_effective))
        if (hasRuleKind(saved.text, "CELLULAR-RADIO")) RuleWarning(stringResource(R.string.rules_radio_not_effective))
        Note(stringResource(R.string.rules_mine_note))
    }
}

@Composable
private fun GroupsSection(groups: List<NativeGroup>, enabled: Boolean, onSelect: (String, String) -> Unit) {
    val title = stringResource(R.string.rules_groups)
    Column {
        Section(
            if (groups.isEmpty()) {
                title
            } else {
                title + " · " + pluralStringResource(R.plurals.rules_group_count, groups.size, groups.size)
            },
        ) {
            if (groups.isEmpty()) {
                Text(
                    stringResource(R.string.rules_no_groups),
                    style = MaterialTheme.typography.bodyMedium,
                    color = LocalBrandPalette.current.text2,
                    modifier = Modifier.padding(BrandSpacing.md),
                )
            }
            groups.forEachIndexed { index, group ->
                if (index > 0) RowDivider()
                GroupRow(group, enabled) { onSelect(group.tag, it) }
            }
        }
        Note(stringResource(R.string.rules_groups_note))
    }
}

/** A policy group; a selector's member is picked from a menu, other kinds show the member they chose. */
@Composable
private fun GroupRow(group: NativeGroup, enabled: Boolean, onSelect: (String) -> Unit) {
    val palette = LocalBrandPalette.current
    val selector = group.groupType == "selector"
    var open by remember { mutableStateOf(false) }
    val kind = when (group.groupType) {
        "selector" -> stringResource(R.string.group_selector)
        "urltest" -> stringResource(R.string.group_urltest)
        else -> group.groupType
    }
    Box {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .then(if (selector && enabled) Modifier.clickable(role = Role.DropdownList) { open = true } else Modifier)
                .heightIn(min = RowMinHeight)
                .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.xs),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(BrandSpacing.md),
        ) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
                Text(group.tag, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(kind, style = MaterialTheme.typography.bodySmall, color = palette.text2)
            }
            Text(
                group.label(group.selected) ?: stringResource(R.string.group_none),
                style = MaterialTheme.typography.bodyLarge,
                color = if (selector && enabled) palette.primary else palette.text2,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f, fill = false),
            )
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            group.outbounds.forEach { outbound ->
                DropdownMenuItem(
                    text = { Text(group.label(outbound) ?: outbound) },
                    trailingIcon = {
                        if (outbound == group.selected) Icon(Icons.Filled.Check, contentDescription = null)
                    },
                    onClick = {
                        open = false
                        if (outbound != group.selected) onSelect(outbound)
                    },
                )
            }
        }
    }
}

@Composable
private fun RuleSetsSection(sets: List<RuleSetStatus>, enabled: Boolean, onUpdate: () -> Unit) {
    Column {
        Section(
            stringResource(R.string.rules_sets),
            action = {
                if (sets.any { it.source == "remote" }) {
                    TextButton(
                        onClick = onUpdate,
                        enabled = enabled,
                    ) {
                        Text(stringResource(R.string.rules_sets_update))
                    }
                }
            },
        ) {
            sets.forEachIndexed { index, set ->
                if (index > 0) RowDivider()
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = RowMinHeight)
                        .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.xs),
                    verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs),
                ) {
                    Text(set.name, style = MaterialTheme.typography.bodyLarge)
                    Text(ruleSetDetail(set), style = MaterialTheme.typography.bodySmall, color = LocalBrandPalette.current.text2)
                    if (set.skippedCount > 0) {
                        Text(
                            pluralStringResource(R.plurals.rule_set_skipped, set.skippedCount, set.skippedCount, set.skippedSummary),
                            style = MaterialTheme.typography.bodySmall,
                            color = LocalBrandPalette.current.warning,
                        )
                    }
                }
            }
        }
        Note(stringResource(R.string.rules_sets_note))
    }
}

/**
 * What importing the subscription's rules left out, so a rule that does not run is never silent: each line's kind in
 * the app's language, then the kernel's English line, which names the rule or group.
 */
@Composable
private fun RoutingWarningsSection(warnings: List<String>) {
    val context = LocalContext.current
    val texts = remember(context) { CodeTexts(context) }
    Column {
        Section(stringResource(R.string.rules_import_warnings)) {
            warnings.forEachIndexed { index, warning ->
                if (index > 0) RowDivider()
                val (kind, detail) = texts.routingWarning(warning)
                Column(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = RowMinHeight)
                        .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.xs),
                ) {
                    Text(kind, style = MaterialTheme.typography.bodySmall, color = LocalBrandPalette.current.warning)
                    if (detail != null) {
                        Text(
                            detail,
                            style = MaterialTheme.typography.bodySmall,
                            color = LocalBrandPalette.current.text2,
                        )
                    }
                }
            }
        }
        Note(stringResource(R.string.rules_import_warnings_note))
    }
}

@Composable
private fun ruleSetDetail(set: RuleSetStatus): String {
    val locale = LocalConfiguration.current.locales[0]
    val remote = set.source == "remote"
    val updatedAt = set.updatedAtMs
    val lastError = set.lastError
    return listOfNotNull(
        stringResource(if (remote) R.string.rule_set_remote else R.string.rule_set_builtin),
        pluralStringResource(R.plurals.rule_set_entries, set.entries, set.entries),
        when {
            updatedAt != null -> stringResource(R.string.rule_set_updated, formatTime(updatedAt, locale))
            remote -> stringResource(R.string.rule_set_not_downloaded)
            else -> null
        },
        lastError?.let { stringResource(R.string.rule_set_failed, it) },
    ).joinToString(" · ")
}

@Composable
private fun DnsSection(snapshot: RuntimeSnapshot?, dns: DnsText?, enabled: Boolean, actions: RulesActions) {
    val current = snapshot?.dnsPreset ?: "system"
    Section(stringResource(R.string.dns_title)) {
        Column(Modifier.selectableGroup()) {
            DnsChoices.forEachIndexed { index, choice ->
                if (index > 0) RowDivider()
                ChoiceRow(
                    stringResource(choice.label),
                    stringResource(choice.hint),
                    selected = choice.value == current,
                    enabled = enabled,
                    onClick = { if (choice.value != current) actions.setDnsPreset(choice.value) },
                )
            }
        }
        if (dns != null) {
            RowDivider()
            SwitchRow(
                stringResource(R.string.dns_fake_ip),
                stringResource(R.string.dns_fake_ip_hint),
                checked = dns.fakeIp,
                enabled = enabled,
                onChange = { actions.saveDns(dns.copy(fakeIp = it)) },
            )
        }
    }
}

/** The 自定义 choice's servers, per-domain servers and hosts, saved together. */
@Composable
private fun CustomDnsSection(saved: DnsText, enabled: Boolean, onSave: (DnsText) -> Unit) {
    var servers by rememberSaveable(saved.servers) { mutableStateOf(saved.servers) }
    var rules by rememberSaveable(saved.rules) { mutableStateOf(saved.rules) }
    var hosts by rememberSaveable(saved.hosts) { mutableStateOf(saved.hosts) }
    val changed = servers != saved.servers || rules != saved.rules || hosts != saved.hosts
    Column {
        Section(stringResource(R.string.dns_custom)) {
            Column(
                modifier = Modifier.padding(BrandSpacing.md),
                verticalArrangement = Arrangement.spacedBy(BrandSpacing.md),
            ) {
                LabelledEditor(R.string.dns_servers, R.string.dns_servers_hint, servers, { servers = it }, ServersPlaceholder)
                LabelledEditor(R.string.dns_rules, R.string.dns_rules_hint, rules, { rules = it }, DnsRulesPlaceholder)
                LabelledEditor(R.string.dns_hosts, R.string.dns_hosts_hint, hosts, { hosts = it }, HostsPlaceholder)
            }
            SaveRow(
                changed,
                enabled,
                onRevert = {
                    servers = saved.servers
                    rules = saved.rules
                    hosts = saved.hosts
                },
                onSave = { onSave(saved.copy(servers = servers, rules = rules, hosts = hosts)) },
            )
        }
        Note(stringResource(R.string.dns_custom_note))
    }
}

@Composable
private fun LabelledEditor(
    @StringRes label: Int,
    @StringRes hint: Int,
    value: String,
    onChange: (String) -> Unit,
    placeholder: String,
) {
    val name = stringResource(label)
    Column(verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
        Text(name, style = MaterialTheme.typography.titleSmall)
        Text(stringResource(hint), style = MaterialTheme.typography.bodySmall, color = LocalBrandPalette.current.text2)
        Editor(value, onChange, name, placeholder)
    }
}

/** A multi-line monospace field for rule-like lines. */
@Composable
private fun Editor(value: String, onChange: (String) -> Unit, label: String, placeholder: String, modifier: Modifier = Modifier) {
    OutlinedTextField(
        value = value,
        onValueChange = onChange,
        minLines = EditorLines,
        textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
        placeholder = {
            Text(placeholder, style = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace))
        },
        label = { Text(label) },
        modifier = modifier.fillMaxWidth(),
    )
}

private const val EditorLines = 4

/** 还原 and 保存 under an editor, both off until the text changes. */
@Composable
private fun SaveRow(changed: Boolean, enabled: Boolean, onRevert: () -> Unit, onSave: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(start = BrandSpacing.md, end = BrandSpacing.md, bottom = BrandSpacing.sm),
        horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs, Alignment.End),
    ) {
        TextButton(onClick = onRevert, enabled = changed) { Text(stringResource(R.string.action_revert)) }
        Button(onClick = onSave, enabled = changed && enabled) { Text(stringResource(R.string.action_save)) }
    }
}

/** One option of a one-of list, with its sentence under the name and a check when chosen. */
@Composable
internal fun ChoiceRow(label: String, hint: String, selected: Boolean, enabled: Boolean, onClick: () -> Unit) {
    val palette = LocalBrandPalette.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .selectable(selected = selected, enabled = enabled, role = Role.RadioButton, onClick = onClick)
            .heightIn(min = RowMinHeight)
            .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.xs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
            Text(label, style = MaterialTheme.typography.bodyLarge)
            Text(hint, style = MaterialTheme.typography.bodySmall, color = palette.text2)
        }
        if (selected) Icon(Icons.Filled.Check, contentDescription = null, tint = palette.primary)
    }
}

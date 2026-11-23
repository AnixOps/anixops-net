package io.anixops.net.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.foundation.clickable
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import io.anixops.net.R
import io.anixops.net.core.DebugMapping
import io.anixops.net.core.DecryptDomain
import io.anixops.net.core.MitmHostOutcome
import io.anixops.net.core.MitmPlugin
import io.anixops.net.core.MitmPluginDetails
import io.anixops.net.core.RedirectDraft
import io.anixops.net.core.RuntimeSnapshot
import io.anixops.net.core.TrafficRedirect
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette

/** What 扩展 asks of the app. */
data class ExtensionsActions(
    val enableDecryption: () -> Unit = {},
    val disableDecryption: () -> Unit = {},
    val confirmDecryption: () -> Unit = {},
    /** The system's security settings, where the user installs the root certificate saved to Downloads. */
    val openSecuritySettings: () -> Unit = {},
    /** Adds a line to the decryption list; true adds it as an exclusion. */
    val addHost: (String, Boolean) -> Unit = { _, _ -> },
    val editHost: (pattern: String, host: String, exclude: Boolean) -> Unit = { _, _, _ -> },
    val setHostEnabled: (pattern: String, enabled: Boolean) -> Unit = { _, _ -> },
    val removeHost: (String) -> Unit = {},
    /** Forgets the engine's decryption results. */
    val clearOutcomes: () -> Unit = {},
    val importPlugin: (String) -> Unit = {},
    val importPluginText: (text: String, source: String) -> Unit = { _, _ -> },
    /** Opens the system's file picker; the chosen file is imported as a plugin. */
    val pickPluginFile: () -> Unit = {},
    val setPluginEnabled: (id: String, enabled: Boolean) -> Unit = { _, _ -> },
    val removePlugin: (String) -> Unit = {},
    val inspect: (MitmPlugin) -> Unit = {},
    val closeInspector: () -> Unit = {},
    /** Opens the mapping editor; null starts a new mapping. */
    val editMapping: (DebugMapping?) -> Unit = {},
    val closeMappingEditor: () -> Unit = {},
    /** The open mapping editor's unsaved fields, kept by the view model. */
    val mappingDraft: () -> MappingDraft? = { null },
    val saveMapping: (DebugMapping) -> Unit = {},
    val setMappingEnabled: (DebugMapping, Boolean) -> Unit = { _, _ -> },
    val removeMapping: (String) -> Unit = {},
    /** Opens the rewrite editor; null starts a new rewrite. */
    val editRedirect: (TrafficRedirect?) -> Unit = {},
    val closeRedirectEditor: () -> Unit = {},
    val saveRedirect: (id: String?, draft: RedirectDraft) -> Unit = { _, _ -> },
    val setRedirectEnabled: (id: String, enabled: Boolean) -> Unit = { _, _ -> },
    val removeRedirect: (String) -> Unit = {},
)

/** A dialog the page has open. */
private sealed interface ExtensionsDialog {
    data object Consent : ExtensionsDialog

    data object ImportUrl : ExtensionsDialog

    data object Paste : ExtensionsDialog

    data class RemovePlugin(val plugin: MitmPlugin) : ExtensionsDialog

    data class RemoveMapping(val id: String, val label: String) : ExtensionsDialog

    data class RemoveRedirect(val id: String, val label: String) : ExtensionsDialog
}

/**
 * 扩展 in proxy mode, 改写 in debug mode ([title]): HTTPS decryption with its first-time confirmation and the root
 * certificate's installation, the domains decrypted one by one, and the plugins.
 */
@Composable
fun ExtensionsScreen(
    title: String,
    snapshot: RuntimeSnapshot?,
    state: ExtensionsState,
    actions: ExtensionsActions,
    modifier: Modifier = Modifier,
) {
    state.editingMapping?.let { mapping ->
        BackTo(actions.closeMappingEditor)
        MappingEditorScreen(
            mapping,
            state.busy,
            onSave = actions.saveMapping,
            onBack = actions.closeMappingEditor,
            modifier = modifier,
            editorDraft = actions.mappingDraft(),
        )
        return
    }
    var dialog by rememberSaveable(stateSaver = ExtensionsDialogSaver) { mutableStateOf<ExtensionsDialog?>(null) }
    val enabled = !state.busy && snapshot != null
    Page(title, modifier) {
        DecryptionSection(
            decrypting = snapshot?.httpsDecryptionEnabled == true,
            enabled = enabled,
            onToggle = { on ->
                when {
                    !on -> actions.disableDecryption()
                    state.consented -> actions.enableDecryption()
                    else -> dialog = ExtensionsDialog.Consent
                }
            },
            openSecuritySettings = actions.openSecuritySettings,
        )
        HostsSection(state, enabled = !state.busy, actions = actions)
        PluginsSection(
            state,
            actions,
            onImportUrl = { dialog = ExtensionsDialog.ImportUrl },
            onPaste = { dialog = ExtensionsDialog.Paste },
            onRemove = { dialog = ExtensionsDialog.RemovePlugin(it) },
        )
        MappingsSection(
            state.mappings,
            loaded = state.loaded,
            enabled = !state.busy,
            onNew = { actions.editMapping(null) },
            onEdit = actions.editMapping,
            onToggle = actions.setMappingEnabled,
            onRemove = { dialog = ExtensionsDialog.RemoveMapping(it.id, mappingLabel(it)) },
        )
        val redirectLabels = state.redirects.associate { it.id to redirectLabel(it) }
        RedirectsSection(
            state.redirects,
            loaded = state.loaded,
            enabled = !state.busy,
            onNew = { actions.editRedirect(null) },
            onEdit = actions.editRedirect,
            onToggle = { entry, on -> actions.setRedirectEnabled(entry.id, on) },
            onRemove = { dialog = ExtensionsDialog.RemoveRedirect(it.id, redirectLabels[it.id].orEmpty()) },
        )
    }
    val dismiss = { dialog = null }
    when (val open = dialog) {
        ExtensionsDialog.Consent -> ConsentDialog(
            onConfirm = {
                dismiss()
                actions.confirmDecryption()
                actions.enableDecryption()
            },
            onDismiss = dismiss,
        )
        ExtensionsDialog.ImportUrl -> TextDialog(
            stringResource(R.string.plugin_import),
            stringResource(R.string.plugin_url),
            "",
            onConfirm = {
                dismiss()
                actions.importPlugin(it)
            },
            onDismiss = dismiss,
            confirm = R.string.action_import,
        )
        ExtensionsDialog.Paste -> PasteDialog(
            onImport = {
                dismiss()
                actions.importPluginText(it, "")
            },
            onDismiss = dismiss,
        )
        is ExtensionsDialog.RemovePlugin -> ConfirmDialog(
            stringResource(R.string.plugin_confirm_remove, open.plugin.name),
            onConfirm = {
                dismiss()
                actions.removePlugin(open.plugin.id)
            },
            onDismiss = dismiss,
        )
        is ExtensionsDialog.RemoveMapping -> ConfirmDialog(
            stringResource(R.string.plugin_confirm_remove, open.label),
            onConfirm = {
                dismiss()
                actions.removeMapping(open.id)
            },
            onDismiss = dismiss,
        )
        is ExtensionsDialog.RemoveRedirect -> ConfirmDialog(
            stringResource(R.string.plugin_confirm_remove, open.label),
            onConfirm = {
                dismiss()
                actions.removeRedirect(open.id)
            },
            onDismiss = dismiss,
        )
        null -> Unit
    }
    state.editingRedirect?.let { editor ->
        RedirectDialog(
            editor,
            state.busy,
            onSave = { actions.saveRedirect(editor.id, it) },
            onDismiss = actions.closeRedirectEditor,
        )
    }
    state.inspected?.let { (plugin, details) -> PluginInspector(plugin, details, actions.closeInspector) }
}

/** Only the plugin dialogs' kind survives a configuration change; a removal question is asked again. */
private val ExtensionsDialogSaver = Saver<ExtensionsDialog?, String>(
    save = {
        when (it) {
            ExtensionsDialog.Consent -> "consent"
            ExtensionsDialog.ImportUrl -> "url"
            ExtensionsDialog.Paste -> "paste"
            else -> ""
        }
    },
    restore = {
        when (it) {
            "consent" -> ExtensionsDialog.Consent
            "url" -> ExtensionsDialog.ImportUrl
            "paste" -> ExtensionsDialog.Paste
            else -> null
        }
    },
)

@Composable
private fun DecryptionSection(
    decrypting: Boolean,
    enabled: Boolean,
    onToggle: (Boolean) -> Unit,
    openSecuritySettings: () -> Unit,
) {
    Column {
        Section(stringResource(R.string.decryption)) {
            SwitchRow(
                stringResource(R.string.decryption_switch),
                stringResource(R.string.decryption_switch_hint),
                checked = decrypting,
                enabled = enabled,
                onChange = onToggle,
            )
            RowDivider()
            // The steps are long: the button goes under them rather than squeezing them.
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.sm),
                verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs),
            ) {
                Text(stringResource(R.string.decryption_root), style = MaterialTheme.typography.bodyLarge)
                Text(
                    stringResource(R.string.decryption_root_hint),
                    style = MaterialTheme.typography.bodySmall,
                    color = LocalBrandPalette.current.text2,
                )
                OutlinedButton(
                    onClick = openSecuritySettings,
                    modifier = Modifier.padding(top = BrandSpacing.xs),
                ) { Text(stringResource(R.string.decryption_root_open)) }
            }
        }
        Note(stringResource(R.string.decryption_note))
    }
}

/**
 * The decryption list: domains and wildcard patterns to decrypt, exclusions that win over every include, each line
 * on or paused and edited in a dialog. While the engine runs, the domains whose decryption failed are listed with the
 * reason (certificate pinning first) and a one-tap exclusion. The list starts empty: nothing is bundled.
 */
@Composable
private fun HostsSection(state: ExtensionsState, enabled: Boolean, actions: ExtensionsActions) {
    var typed by rememberSaveable { mutableStateOf("") }
    var exclude by rememberSaveable { mutableStateOf(false) }
    // The open edit dialog survives a rotation and another page: its pattern is kept and looked up again.
    val domains by rememberUpdatedState(state.domains)
    var editing by rememberSaveable(
        stateSaver = Saver<DecryptDomain?, String>(
            save = { it?.pattern },
            restore = { pattern -> domains.firstOrNull { it.pattern == pattern } },
        ),
    ) { mutableStateOf<DecryptDomain?>(null) }
    val add = {
        actions.addHost(typed, exclude)
        typed = ""
    }
    val count = state.hosts.size
    val failing = state.outcomes.filter { it.failing }
    val excluded = state.domains.filter { it.enabled && it.exclude }.map { it.pattern }
    Column {
        Section(
            stringResource(R.string.decrypt_hosts) +
                if (count == 0) "" else " · " + pluralStringResource(R.plurals.decrypt_host_count, count, count),
        ) {
            Row(
                modifier = Modifier.padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.sm),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
            ) {
                OutlinedTextField(
                    value = typed,
                    onValueChange = { typed = it },
                    singleLine = true,
                    label = { Text(stringResource(R.string.decrypt_host_field)) },
                    placeholder = { Text(stringResource(R.string.decrypt_host_example)) },
                    modifier = Modifier.weight(1f),
                )
                TextButton(onClick = add, enabled = enabled && typed.isNotBlank()) { Text(stringResource(R.string.action_add)) }
            }
            KindChoice(
                exclude = exclude,
                onChange = { exclude = it },
                modifier = Modifier.padding(start = BrandSpacing.md, end = BrandSpacing.md, bottom = BrandSpacing.sm),
            )
            (state.domains.filter { !it.exclude } + state.domains.filter { it.exclude }).forEach { domain ->
                RowDivider()
                DomainRow(
                    domain = domain,
                    status = domainStatus(domain, state.outcomes),
                    enabled = enabled,
                    onEdit = { editing = domain },
                    onToggle = { actions.setHostEnabled(domain.pattern, it) },
                    onRemove = { actions.removeHost(domain.pattern) },
                )
            }
        }
        Note(stringResource(R.string.decrypt_hosts_note))
        if (failing.isNotEmpty()) {
            Section(
                stringResource(R.string.decrypt_failed),
                modifier = Modifier.padding(top = BrandSpacing.md),
                action = { TextButton(onClick = actions.clearOutcomes) { Text(stringResource(R.string.decrypt_failed_clear)) } },
            ) {
                failing.forEachIndexed { index, outcome ->
                    if (index > 0) RowDivider()
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .heightIn(min = RowMinHeight)
                            .padding(start = BrandSpacing.md, top = BrandSpacing.xs, bottom = BrandSpacing.xs),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
                            Text(outcome.host, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            Text(
                                stringResource(failureReason(outcome.lastFailure)) + " · " +
                                    stringResource(R.string.decrypt_failed_counts, outcome.failed, outcome.decrypted),
                                style = MaterialTheme.typography.bodySmall,
                                color = LocalBrandPalette.current.text2,
                            )
                        }
                        if (excluded.any { patternMatches(it, outcome.host) }) {
                            Text(
                                stringResource(R.string.decrypt_excluded),
                                style = MaterialTheme.typography.bodySmall,
                                color = LocalBrandPalette.current.text2,
                                modifier = Modifier.padding(end = BrandSpacing.md),
                            )
                        } else {
                            TextButton(onClick = { actions.addHost(outcome.host, true) }, enabled = enabled) {
                                Text(stringResource(R.string.decrypt_exclude))
                            }
                        }
                    }
                }
            }
            Note(stringResource(R.string.decrypt_failed_note))
        }
        Note(stringResource(R.string.decrypt_what_works))
    }
    editing?.let { domain ->
        EditDomainDialog(
            domain = domain,
            onSave = { text, asExclusion ->
                actions.editHost(domain.pattern, text, asExclusion)
                editing = null
            },
            onDismiss = { editing = null },
        )
    }
}

@Composable
private fun KindChoice(exclude: Boolean, onChange: (Boolean) -> Unit, modifier: Modifier = Modifier) {
    val kinds = listOf(false to R.string.decrypt_kind_decrypt, true to R.string.decrypt_kind_exclude)
    SingleChoiceSegmentedButtonRow(modifier = modifier.fillMaxWidth()) {
        kinds.forEachIndexed { index, (value, label) ->
            SegmentedButton(
                selected = exclude == value,
                onClick = { onChange(value) },
                shape = SegmentedButtonDefaults.itemShape(index, kinds.size),
            ) { Text(stringResource(label)) }
        }
    }
}

@Composable
private fun DomainRow(
    domain: DecryptDomain,
    status: String?,
    enabled: Boolean,
    onEdit: () -> Unit,
    onToggle: (Boolean) -> Unit,
    onRemove: () -> Unit,
) {
    val palette = LocalBrandPalette.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = RowMinHeight)
            .padding(start = BrandSpacing.md),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs),
    ) {
        Column(
            Modifier
                .weight(1f)
                .clickable(enabled = enabled, onClick = onEdit)
                .padding(vertical = BrandSpacing.xs),
            verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs),
        ) {
            Text(
                if (domain.exclude) "-" + domain.pattern else domain.pattern,
                style = MaterialTheme.typography.bodyLarge,
                fontFamily = FontFamily.Monospace,
                color = if (domain.enabled) palette.text else palette.text2,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            when {
                !domain.enabled -> Text(stringResource(R.string.decrypt_paused), style = MaterialTheme.typography.bodySmall, color = palette.text2)
                status != null -> Text(status, style = MaterialTheme.typography.bodySmall, color = palette.text2)
            }
        }
        Switch(checked = domain.enabled, onCheckedChange = onToggle, enabled = enabled)
        IconButton(onClick = onRemove, enabled = enabled) {
            Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.decrypt_host_remove, domain.pattern))
        }
    }
}

@Composable
private fun EditDomainDialog(domain: DecryptDomain, onSave: (String, Boolean) -> Unit, onDismiss: () -> Unit) {
    var text by rememberSaveable(domain.pattern) { mutableStateOf(domain.pattern) }
    var exclude by rememberSaveable(domain.pattern) { mutableStateOf(domain.exclude) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.decrypt_edit)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(BrandSpacing.sm)) {
                OutlinedTextField(
                    value = text,
                    onValueChange = { text = it },
                    singleLine = true,
                    label = { Text(stringResource(R.string.decrypt_host_field)) },
                )
                KindChoice(exclude = exclude, onChange = { exclude = it })
            }
        },
        confirmButton = {
            TextButton(onClick = { onSave(text, exclude) }, enabled = text.isNotBlank()) { Text(stringResource(R.string.action_save)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

/** What the engine saw for the domains a line names: decrypted connections and how many domains fail. */
@Composable
private fun domainStatus(domain: DecryptDomain, outcomes: List<MitmHostOutcome>): String? {
    if (domain.exclude || !domain.enabled) return null
    val seen = outcomes.filter { patternMatches(domain.pattern, it.host) }
    if (seen.isEmpty()) return null
    val decrypted = seen.sumOf { it.decrypted }
    val failing = seen.count { it.failing }
    return if (failing > 0) {
        stringResource(R.string.decrypt_status_failing, decrypted, failing)
    } else {
        stringResource(R.string.decrypt_status, decrypted)
    }
}

private fun failureReason(code: String?): Int = when (code) {
    "client_rejected_certificate" -> R.string.decrypt_reason_rejected
    "client_closed" -> R.string.decrypt_reason_closed
    "client_handshake_failed" -> R.string.decrypt_reason_handshake
    "client_timeout" -> R.string.decrypt_reason_timeout
    "origin_tls" -> R.string.decrypt_reason_origin_tls
    "origin_unreachable" -> R.string.decrypt_reason_origin_unreachable
    else -> R.string.decrypt_failed
}

/** Whether [pattern] matches [host]: `*` any run of characters (dots too), `?` one, as the engine reads them. */
internal fun patternMatches(pattern: String, host: String): Boolean {
    val p = pattern.trim().lowercase()
    val text = host.trimEnd('.').lowercase()
    var pi = 0
    var ti = 0
    var star = -1
    var mark = 0
    while (ti < text.length) {
        if (pi < p.length && (p[pi] == '?' || p[pi] == text[ti])) {
            pi++
            ti++
        } else if (pi < p.length && p[pi] == '*') {
            star = pi
            mark = ti
            pi++
        } else if (star >= 0) {
            pi = star + 1
            mark++
            ti = mark
        } else {
            return false
        }
    }
    while (pi < p.length && p[pi] == '*') pi++
    return pi == p.length
}

@Composable
private fun PluginsSection(
    state: ExtensionsState,
    actions: ExtensionsActions,
    onImportUrl: () -> Unit,
    onPaste: () -> Unit,
    onRemove: (MitmPlugin) -> Unit,
) {
    val enabled = !state.busy
    val plugins = state.plugins
    Column {
        Section(
            stringResource(R.string.plugins) +
                if (plugins.isEmpty()) "" else " · " + pluralStringResource(R.plurals.plugin_count, plugins.size, plugins.size),
            action = {
                Row {
                    TextButton(onClick = onImportUrl, enabled = enabled) { Text(stringResource(R.string.plugin_import)) }
                    OverflowMenu(stringResource(R.string.plugin_more_imports), enabled) { close ->
                        MenuItem(R.string.plugin_import_file, close, actions.pickPluginFile)
                        MenuItem(R.string.plugin_paste, close, onPaste)
                    }
                }
            },
        ) {
            if (state.loaded && plugins.isEmpty()) {
                Column(
                    modifier = Modifier.padding(BrandSpacing.md),
                    verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs),
                ) {
                    Text(stringResource(R.string.plugins_empty), style = MaterialTheme.typography.titleMedium)
                    Text(
                        stringResource(R.string.plugins_empty_body),
                        style = MaterialTheme.typography.bodyMedium,
                        color = LocalBrandPalette.current.text2,
                    )
                }
            }
            plugins.forEachIndexed { index, plugin ->
                if (index > 0) RowDivider()
                PluginLine(
                    plugin,
                    enabled = enabled,
                    onToggle = { actions.setPluginEnabled(plugin.id, it) },
                    onInspect = { actions.inspect(plugin) },
                    onRemove = { onRemove(plugin) },
                )
            }
        }
        Note(stringResource(R.string.plugins_note))
    }
}

@Composable
private fun PluginLine(
    plugin: MitmPlugin,
    enabled: Boolean,
    onToggle: (Boolean) -> Unit,
    onInspect: () -> Unit,
    onRemove: () -> Unit,
) {
    val palette = LocalBrandPalette.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = RowMinHeight)
            .padding(start = BrandSpacing.md, end = BrandSpacing.md, top = BrandSpacing.sm, bottom = BrandSpacing.sm),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
            Row(horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    plugin.name,
                    style = MaterialTheme.typography.bodyLarge,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
                when (plugin.level) {
                    "full" -> Unit
                    "partial" -> Text(
                        stringResource(R.string.plugin_partial),
                        style = MaterialTheme.typography.labelMedium,
                        color = palette.warning,
                    )
                    else -> Text(
                        stringResource(R.string.plugin_unsupported),
                        style = MaterialTheme.typography.labelMedium,
                        color = palette.danger,
                    )
                }
            }
            Text(
                pluginDetail(plugin),
                style = MaterialTheme.typography.bodySmall,
                color = palette.text2,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
        OverflowMenu(stringResource(R.string.proxies_more, plugin.name), enabled) { close ->
            MenuItem(R.string.plugin_view, close, onInspect)
            MenuItem(R.string.plugin_remove, close, onRemove)
        }
        Switch(checked = plugin.enabled, onCheckedChange = onToggle, enabled = enabled)
    }
}

/** The plugin's first three domains and how many scripts it runs. */
@Composable
private fun pluginDetail(plugin: MitmPlugin): String {
    val hosts = plugin.hostnames
    val domains = when {
        hosts.isEmpty() -> stringResource(R.string.plugin_no_hosts)
        hosts.size > PLUGIN_HOSTS_SHOWN ->
            hosts.take(PLUGIN_HOSTS_SHOWN).joinToString(", ") + " " +
                pluralStringResource(R.plurals.plugin_more_hosts, hosts.size, hosts.size)
        else -> hosts.joinToString(", ")
    }
    val scripts = if (plugin.scriptCount > 0) {
        pluralStringResource(R.plurals.plugin_scripts, plugin.scriptCount, plugin.scriptCount)
    } else {
        stringResource(R.string.plugin_no_scripts)
    }
    return "$domains · $scripts"
}

private const val PLUGIN_HOSTS_SHOWN = 3

/** What one plugin holds: the domains it decrypts, its scripts and files, and how much of it the engine runs. */
@Composable
private fun PluginInspector(plugin: MitmPlugin, details: MitmPluginDetails, onClose: () -> Unit) {
    val lines = buildList {
        add(pluralStringResource(R.plurals.plugin_lines_supported, details.supportedLines, details.supportedLines))
        if (details.partialLines > 0) {
            add(pluralStringResource(R.plurals.plugin_lines_partial, details.partialLines, details.partialLines))
        }
        if (details.unsupportedLines > 0) {
            add(pluralStringResource(R.plurals.plugin_lines_unsupported, details.unsupportedLines, details.unsupportedLines))
        }
        if (details.deferredLines > 0) {
            add(pluralStringResource(R.plurals.plugin_lines_deferred, details.deferredLines, details.deferredLines))
        }
    }.joinToString(" · ")
    AlertDialog(
        onDismissRequest = onClose,
        title = { Text(plugin.name) },
        text = {
            Column(
                modifier = Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
            ) {
                Text(
                    plugin.url.ifEmpty { stringResource(R.string.plugin_local) },
                    style = MaterialTheme.typography.bodySmall,
                    color = LocalBrandPalette.current.text2,
                )
                Fact(stringResource(R.string.plugin_lines), lines)
                Fact(
                    stringResource(R.string.decrypt_hosts),
                    plugin.hostnames.joinToString(", ").ifEmpty { stringResource(R.string.plugin_no_hosts) },
                )
                if (details.excludedHostnames.isNotEmpty()) {
                    Fact(stringResource(R.string.plugin_excluded), details.excludedHostnames.joinToString(", "))
                }
                Fact(
                    stringResource(R.string.plugin_script_list),
                    details.scripts.joinToString("\n").ifEmpty { stringResource(R.string.plugin_no_scripts) },
                )
                if (details.resources.isNotEmpty()) {
                    Fact(stringResource(R.string.plugin_resources), details.resources.joinToString("\n"))
                }
                if (details.report.isNotBlank()) {
                    Text(details.report, style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace)
                }
            }
        },
        confirmButton = { TextButton(onClick = onClose) { Text(stringResource(R.string.action_close)) } },
    )
}

@Composable
private fun Fact(label: String, value: String) {
    Column(verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
        Text(label, style = MaterialTheme.typography.labelMedium, color = LocalBrandPalette.current.text2)
        Text(value, style = MaterialTheme.typography.bodyMedium)
    }
}

@Composable
private fun PasteDialog(onImport: (String) -> Unit, onDismiss: () -> Unit) {
    var text by rememberSaveable { mutableStateOf("") }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.plugin_paste)) },
        text = {
            OutlinedTextField(
                value = text,
                onValueChange = { text = it },
                label = { Text(stringResource(R.string.plugin_text)) },
                placeholder = { Text("#!name = …\n[Rewrite]\n…\n[MITM]\nhostname = …", fontFamily = FontFamily.Monospace) },
                textStyle = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace),
                minLines = 6,
                maxLines = 12,
                modifier = Modifier.fillMaxWidth(),
            )
        },
        confirmButton = {
            TextButton(onClick = { onImport(text) }, enabled = text.isNotBlank()) { Text(stringResource(R.string.action_import)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

/** Asked the first time decryption is turned on (TERMS.md §4), with the same four points as the other clients. */
@Composable
private fun ConsentDialog(onConfirm: () -> Unit, onDismiss: () -> Unit) {
    val points = listOf(
        R.string.decryption_consent_own,
        R.string.decryption_consent_others,
        R.string.decryption_consent_secrets,
        R.string.decryption_consent_abuse,
    )
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.decryption_consent_title)) },
        text = {
            Column(
                modifier = Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
            ) {
                Text(stringResource(R.string.decryption_consent_intro), style = MaterialTheme.typography.bodyMedium)
                points.forEach { point -> BulletLine(stringResource(point)) }
            }
        },
        confirmButton = { TextButton(onClick = onConfirm) { Text(stringResource(R.string.decryption_consent_confirm)) } },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

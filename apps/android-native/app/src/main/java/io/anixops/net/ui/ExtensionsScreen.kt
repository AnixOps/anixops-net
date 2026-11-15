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
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
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
import io.anixops.net.core.MitmPlugin
import io.anixops.net.core.MitmPluginDetails
import io.anixops.net.core.RuntimeSnapshot
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette

/** What 扩展 asks of the app. */
data class ExtensionsActions(
    val enableDecryption: () -> Unit = {},
    val disableDecryption: () -> Unit = {},
    val confirmDecryption: () -> Unit = {},
    /** The system's security settings, where the user installs the root certificate saved to Downloads. */
    val openSecuritySettings: () -> Unit = {},
    val addHost: (String) -> Unit = {},
    val removeHost: (String) -> Unit = {},
    val importPlugin: (String) -> Unit = {},
    val importPluginText: (text: String, source: String) -> Unit = { _, _ -> },
    /** Opens the system's file picker; the chosen file is imported as a plugin. */
    val pickPluginFile: () -> Unit = {},
    val setPluginEnabled: (id: String, enabled: Boolean) -> Unit = { _, _ -> },
    val removePlugin: (String) -> Unit = {},
    val inspect: (MitmPlugin) -> Unit = {},
    val closeInspector: () -> Unit = {},
)

/** A dialog the page has open. */
private sealed interface ExtensionsDialog {
    data object Consent : ExtensionsDialog

    data object ImportUrl : ExtensionsDialog

    data object Paste : ExtensionsDialog

    data class RemovePlugin(val plugin: MitmPlugin) : ExtensionsDialog
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
        HostsSection(state.hosts, enabled = !state.busy, onAdd = actions.addHost, onRemove = actions.removeHost)
        PluginsSection(
            state,
            actions,
            onImportUrl = { dialog = ExtensionsDialog.ImportUrl },
            onPaste = { dialog = ExtensionsDialog.Paste },
            onRemove = { dialog = ExtensionsDialog.RemovePlugin(it) },
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
        null -> Unit
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

@Composable
private fun HostsSection(hosts: List<String>, enabled: Boolean, onAdd: (String) -> Unit, onRemove: (String) -> Unit) {
    var typed by rememberSaveable { mutableStateOf("") }
    val add = {
        onAdd(typed)
        typed = ""
    }
    Column {
        Section(
            stringResource(R.string.decrypt_hosts) +
                if (hosts.isEmpty()) "" else " · " + pluralStringResource(R.plurals.decrypt_host_count, hosts.size, hosts.size),
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
            hosts.forEach { host ->
                RowDivider()
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = RowMinHeight)
                        .padding(start = BrandSpacing.md),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        host,
                        style = MaterialTheme.typography.bodyLarge,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )
                    IconButton(onClick = { onRemove(host) }, enabled = enabled) {
                        Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.decrypt_host_remove, host))
                    }
                }
            }
        }
        Note(stringResource(R.string.decrypt_hosts_note))
    }
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

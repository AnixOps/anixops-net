package io.anixops.net.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import io.anixops.net.R
import io.anixops.net.core.NodeForm
import io.anixops.net.core.NodeRow
import io.anixops.net.core.NodeSection
import io.anixops.net.core.SubscriptionRow
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette
import kotlinx.coroutines.launch

/** What the proxies page can ask for. */
data class ProxiesActions(
    val search: (String) -> Unit = {},
    val select: (String) -> Unit = {},
    val testAll: () -> Unit = {},
    val selectFastest: () -> Unit = {},
    val share: (id: String, label: String) -> Unit = { _, _ -> },
    val loadForm: suspend (String) -> NodeForm? = { null },
    val edit: (id: String, name: String, server: String, port: String) -> Unit = { _, _, _, _ -> },
    val remove: (String) -> Unit = {},
    val importSubscription: (String) -> Unit = {},
    val useSubscription: (String) -> Unit = {},
    val updateSubscription: (String) -> Unit = {},
    val renameSubscription: (id: String, name: String) -> Unit = { _, _ -> },
    val setInterval: (id: String, hours: Int) -> Unit = { _, _ -> },
    val removeSubscription: (String) -> Unit = {},
)

/** A dialog the page has open. */
private sealed interface ProxiesDialog {
    data class Import(val scanned: String = "") : ProxiesDialog

    data object Scan : ProxiesDialog

    data class Rename(val subscription: SubscriptionRow) : ProxiesDialog

    data class Interval(val subscription: SubscriptionRow) : ProxiesDialog

    data class RemoveSubscription(val subscription: SubscriptionRow) : ProxiesDialog

    data class Edit(val node: NodeRow, val form: NodeForm) : ProxiesDialog

    data class RemoveNode(val node: NodeRow) : ProxiesDialog
}

/** 代理: subscriptions, then every node by subscription, with search, delay tests and node management. */
@Composable
fun ProxiesScreen(state: ProxiesState, actions: ProxiesActions, modifier: Modifier = Modifier) {
    var dialog by remember { mutableStateOf<ProxiesDialog?>(null) }
    val scope = rememberCoroutineScope()
    Page(stringResource(R.string.nav_proxies), modifier) {
        SubscriptionsSection(
            state.subscriptions,
            enabled = !state.busy,
            actions = actions,
            onImport = { dialog = ProxiesDialog.Import() },
            onRename = { dialog = ProxiesDialog.Rename(it) },
            onInterval = { dialog = ProxiesDialog.Interval(it) },
            onRemove = { dialog = ProxiesDialog.RemoveSubscription(it) },
        )
        val view = state.page.view
        if (state.loaded && view.total == 0) {
            EmptyNodes()
        } else {
            OutlinedTextField(
                value = state.query,
                onValueChange = actions.search,
                singleLine = true,
                leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null) },
                placeholder = { Text(stringResource(R.string.proxies_search)) },
                modifier = Modifier.fillMaxWidth(),
            )
            Row(horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
                OutlinedButton(onClick = actions.testAll, enabled = !state.testing && view.shown > 0) {
                    Text(stringResource(if (state.testing) R.string.proxies_testing else R.string.proxies_test_all))
                }
                OutlinedButton(onClick = actions.selectFastest, enabled = !state.busy && view.total > 0) {
                    Text(stringResource(R.string.proxies_fastest))
                }
            }
            if (state.loaded && view.shown == 0) {
                Text(
                    stringResource(R.string.proxies_no_match),
                    style = MaterialTheme.typography.bodyMedium,
                    color = LocalBrandPalette.current.text2,
                )
            }
            view.sections.forEach { section ->
                Section(
                    sectionTitle(section) + " · " +
                        pluralStringResource(R.plurals.proxies_node_count, section.nodes.size, section.nodes.size),
                ) {
                    section.nodes.forEachIndexed { index, node ->
                        if (index > 0) RowDivider()
                        NodeLine(
                            node,
                            editable = node.id in state.page.editable,
                            enabled = !state.busy,
                            onSelect = { actions.select(node.id) },
                            onShare = { actions.share(node.id, node.label) },
                            onEdit = {
                                scope.launch {
                                    actions.loadForm(node.id)?.let { dialog = ProxiesDialog.Edit(node, it) }
                                }
                            },
                            onRemove = { dialog = ProxiesDialog.RemoveNode(node) },
                        )
                    }
                }
            }
        }
    }
    ProxiesDialogs(dialog, actions, onChange = { dialog = it })
}

/** A subscription's name; pasted links are named in the app's language rather than the core's. */
@Composable
private fun subscriptionName(subscription: SubscriptionRow): String =
    if (subscription.pasted) stringResource(R.string.proxies_pasted) else subscription.name

/** A node section's heading: its subscription, pasted links, or nodes that belong to none. */
@Composable
private fun sectionTitle(section: NodeSection): String = when {
    section.pasted -> stringResource(R.string.proxies_pasted)
    else -> section.group ?: stringResource(R.string.proxies_other_nodes)
}

@Composable
private fun EmptyNodes() {
    Section(title = null) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(BrandSpacing.md),
            verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs),
        ) {
            Text(stringResource(R.string.proxies_empty), style = MaterialTheme.typography.titleMedium)
            Text(
                stringResource(R.string.proxies_empty_body),
                style = MaterialTheme.typography.bodyMedium,
                color = LocalBrandPalette.current.text2,
            )
        }
    }
}

@Composable
private fun SubscriptionsSection(
    subscriptions: List<SubscriptionRow>,
    enabled: Boolean,
    actions: ProxiesActions,
    onImport: () -> Unit,
    onRename: (SubscriptionRow) -> Unit,
    onInterval: (SubscriptionRow) -> Unit,
    onRemove: (SubscriptionRow) -> Unit,
) {
    Section(
        stringResource(R.string.subscriptions),
        action = { TextButton(onClick = onImport, enabled = enabled) { Text(stringResource(R.string.subscription_import)) } },
    ) {
        subscriptions.forEachIndexed { index, subscription ->
            if (index > 0) RowDivider()
            SubscriptionLine(
                subscription,
                enabled = enabled,
                onUse = { actions.useSubscription(subscription.id) },
                onUpdate = { actions.updateSubscription(subscription.id) },
                onRename = { onRename(subscription) },
                onInterval = { onInterval(subscription) },
                onRemove = { onRemove(subscription) },
            )
        }
        if (subscriptions.isEmpty()) {
            Text(
                stringResource(R.string.home_no_subscription),
                style = MaterialTheme.typography.bodyMedium,
                color = LocalBrandPalette.current.text2,
                modifier = Modifier.padding(BrandSpacing.md),
            )
        }
    }
}

@Composable
private fun SubscriptionLine(
    subscription: SubscriptionRow,
    enabled: Boolean,
    onUse: () -> Unit,
    onUpdate: () -> Unit,
    onRename: () -> Unit,
    onInterval: () -> Unit,
    onRemove: () -> Unit,
) {
    val palette = LocalBrandPalette.current
    val locale = LocalConfiguration.current.locales[0]
    val facts = buildList {
        subscription.nodes?.let { add(pluralStringResource(R.plurals.proxies_node_count, it, it)) }
        if (!subscription.pasted) {
            add(
                subscription.updated?.let { stringResource(R.string.subscription_updated_at, it) }
                    ?: stringResource(R.string.subscription_never),
            )
        }
    }
    val usage = buildList {
        val used = subscription.used
        val total = subscription.total
        if (used != null && total != null) {
            add(stringResource(R.string.subscription_used_of, formatBytes(used), formatBytes(total)))
        } else if (used != null) {
            add(stringResource(R.string.subscription_used, formatBytes(used)))
        }
        subscription.expire?.let {
            add(
                if (subscription.expired) stringResource(R.string.subscription_expired)
                else stringResource(R.string.subscription_expires, formatDay(it, locale)),
            )
        }
    }
    val name = subscriptionName(subscription)
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(enabled = enabled && !subscription.selected, onClick = onUse)
            .heightIn(min = RowMinHeight)
            .padding(start = BrandSpacing.md, top = BrandSpacing.sm, bottom = BrandSpacing.sm),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
            Row(horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs), verticalAlignment = Alignment.CenterVertically) {
                Text(
                    name,
                    style = MaterialTheme.typography.bodyLarge,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f, fill = false),
                )
                if (subscription.selected) {
                    Text(stringResource(R.string.proxies_in_use), style = MaterialTheme.typography.labelMedium, color = palette.primary)
                }
            }
            if (facts.isNotEmpty()) {
                Text(facts.joinToString(" · "), style = MaterialTheme.typography.bodySmall, color = palette.text2)
            }
            if (usage.isNotEmpty()) {
                Text(
                    usage.joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall,
                    color = if (subscription.expired) palette.warning else palette.text2,
                )
            }
            subscription.error?.let {
                Text(
                    stringResource(R.string.subscription_error, it),
                    style = MaterialTheme.typography.bodySmall,
                    color = palette.danger,
                )
            }
        }
        OverflowMenu(stringResource(R.string.proxies_more, name), enabled) { close ->
            if (!subscription.selected) MenuItem(R.string.subscription_use, close, onUse)
            if (subscription.refreshable) MenuItem(R.string.subscription_update, close, onUpdate)
            MenuItem(R.string.subscription_rename, close, onRename)
            if (subscription.refreshable) MenuItem(R.string.subscription_interval, close, onInterval)
            MenuItem(R.string.subscription_remove, close, onRemove)
        }
    }
}

@Composable
private fun NodeLine(
    node: NodeRow,
    editable: Boolean,
    enabled: Boolean,
    onSelect: () -> Unit,
    onShare: () -> Unit,
    onEdit: () -> Unit,
    onRemove: () -> Unit,
) {
    val palette = LocalBrandPalette.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .selectable(selected = node.selected, enabled = enabled, role = Role.RadioButton, onClick = onSelect)
            .heightIn(min = RowMinHeight)
            .padding(start = BrandSpacing.md, top = BrandSpacing.xs, bottom = BrandSpacing.xs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
    ) {
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
            Text(node.label, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(node.protocol, style = MaterialTheme.typography.bodySmall, color = palette.text2)
        }
        node.delay?.let { delay ->
            Text(
                delay.ms?.let { stringResource(R.string.proxies_delay_ms, it) } ?: stringResource(R.string.proxies_timed_out),
                style = MaterialTheme.typography.labelMedium,
                color = when (delay.tone) {
                    "fast" -> palette.success
                    "slow" -> palette.warning
                    else -> palette.danger
                },
            )
        }
        if (node.selected) Icon(Icons.Filled.Check, contentDescription = stringResource(R.string.proxies_in_use), tint = palette.primary)
        OverflowMenu(stringResource(R.string.proxies_more, node.label), enabled) { close ->
            MenuItem(R.string.proxies_share, close, onShare)
            if (editable) {
                MenuItem(R.string.proxies_edit, close, onEdit)
                MenuItem(R.string.proxies_remove, close, onRemove)
            }
        }
    }
}

@Composable
private fun ProxiesDialogs(dialog: ProxiesDialog?, actions: ProxiesActions, onChange: (ProxiesDialog?) -> Unit) {
    val dismiss = { onChange(null) }
    when (dialog) {
        null -> Unit
        is ProxiesDialog.Import -> ImportDialog(
            initial = dialog.scanned,
            onScan = { onChange(ProxiesDialog.Scan) },
            onImport = {
                dismiss()
                actions.importSubscription(it)
            },
            onDismiss = dismiss,
        )
        ProxiesDialog.Scan -> QrScanner(
            onResult = { onChange(ProxiesDialog.Import(scanned = it)) },
            onDismiss = { onChange(ProxiesDialog.Import()) },
        )
        is ProxiesDialog.Rename -> TextDialog(
            title = stringResource(R.string.subscription_rename),
            label = stringResource(R.string.node_name),
            initial = subscriptionName(dialog.subscription),
            onConfirm = {
                dismiss()
                actions.renameSubscription(dialog.subscription.id, it)
            },
            onDismiss = dismiss,
        )
        is ProxiesDialog.Interval -> IntervalDialog(dialog.subscription, onChoose = {
            dismiss()
            actions.setInterval(dialog.subscription.id, it)
        }, onDismiss = dismiss)
        is ProxiesDialog.RemoveSubscription -> ConfirmDialog(
            stringResource(R.string.subscription_confirm_remove, subscriptionName(dialog.subscription)),
            onConfirm = {
                dismiss()
                actions.removeSubscription(dialog.subscription.id)
            },
            onDismiss = dismiss,
        )
        is ProxiesDialog.Edit -> EditNodeDialog(dialog.form, onSave = { name, server, port ->
            dismiss()
            actions.edit(dialog.node.id, name, server, port)
        }, onDismiss = dismiss)
        is ProxiesDialog.RemoveNode -> ConfirmDialog(
            stringResource(R.string.proxies_confirm_remove, dialog.node.label),
            onConfirm = {
                dismiss()
                actions.remove(dialog.node.id)
            },
            onDismiss = dismiss,
        )
    }
}

@Composable
private fun ImportDialog(initial: String, onScan: () -> Unit, onImport: (String) -> Unit, onDismiss: () -> Unit) {
    var location by rememberSaveable(initial) { mutableStateOf(initial) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.subscription_import)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(BrandSpacing.sm)) {
                OutlinedTextField(
                    value = location,
                    onValueChange = { location = it },
                    label = { Text(stringResource(R.string.subscription_link)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedButton(onClick = onScan) { Text(stringResource(R.string.subscription_scan)) }
            }
        },
        confirmButton = {
            TextButton(onClick = { onImport(location) }, enabled = location.isNotBlank()) {
                Text(stringResource(R.string.subscription_import_action))
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

@Composable
private fun IntervalDialog(subscription: SubscriptionRow, onChoose: (Int) -> Unit, onDismiss: () -> Unit) {
    val choices = (ProxiesViewModel.INTERVALS + subscription.intervalHours).distinct().sorted()
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.subscription_interval)) },
        text = {
            Column {
                choices.forEach { hours ->
                    Row(
                        modifier = Modifier
                            .fillMaxWidth()
                            .selectable(
                                selected = hours == subscription.intervalHours,
                                role = Role.RadioButton,
                                onClick = { onChoose(hours) },
                            )
                            .heightIn(min = RowMinHeight),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        RadioButton(selected = hours == subscription.intervalHours, onClick = null)
                        Text(intervalLabel(hours), modifier = Modifier.padding(start = BrandSpacing.sm))
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

@Composable
private fun intervalLabel(hours: Int): String = when {
    hours == 0 -> stringResource(R.string.interval_manual)
    hours == WEEK_HOURS -> stringResource(R.string.interval_weekly)
    hours % DAY_HOURS == 0 -> pluralStringResource(R.plurals.interval_days, hours / DAY_HOURS, hours / DAY_HOURS)
    else -> pluralStringResource(R.plurals.interval_hours, hours, hours)
}

private const val DAY_HOURS = 24
private const val WEEK_HOURS = 168

@Composable
private fun EditNodeDialog(form: NodeForm, onSave: (String, String, String) -> Unit, onDismiss: () -> Unit) {
    var name by rememberSaveable { mutableStateOf(form.name) }
    var server by rememberSaveable { mutableStateOf(form.server) }
    var port by rememberSaveable { mutableStateOf(form.port.toString()) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.proxies_edit)) },
        text = {
            Column(verticalArrangement = Arrangement.spacedBy(BrandSpacing.sm)) {
                OutlinedTextField(name, { name = it }, singleLine = true, label = { Text(stringResource(R.string.node_name)) })
                OutlinedTextField(
                    server,
                    { server = it },
                    singleLine = true,
                    label = { Text(stringResource(R.string.node_server)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                )
                OutlinedTextField(
                    port,
                    { port = it.filter(Char::isDigit) },
                    singleLine = true,
                    label = { Text(stringResource(R.string.node_port)) },
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number),
                )
                Text(
                    stringResource(R.string.node_edit_note),
                    style = MaterialTheme.typography.bodySmall,
                    color = LocalBrandPalette.current.text2,
                )
            }
        },
        confirmButton = {
            TextButton(onClick = { onSave(name, server, port) }, enabled = name.isNotBlank() && server.isNotBlank() && port.isNotBlank()) {
                Text(stringResource(R.string.action_save))
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

package io.anixops.net.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
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
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import io.anixops.net.R
import io.anixops.net.core.DebugMapping
import io.anixops.net.core.MappingText
import io.anixops.net.core.RedirectDraft
import io.anixops.net.core.TrafficRedirect
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette

/** A mapping's name in lists: the one the user gave, else its pattern. */
fun mappingLabel(entry: DebugMapping): String = entry.name.trim().ifEmpty { entry.pattern }

/** What a rewrite takes, as the list shows it: the editor's fields when it has them, else the engine's rule. */
@Composable
fun redirectLabel(entry: TrafficRedirect): String {
    val draft = entry.draft ?: return entry.match
    val ports = if (draft.ports.isEmpty()) "" else ":" + draft.ports
    val network = if (draft.network == "any") "" else " " + draft.network.uppercase()
    return draft.source.ifEmpty { stringResource(R.string.redirect_any_address) } + ports + network
}

/** The saved Map Local / Map Remote mappings; a mapping opens in its own editor page. */
@Composable
fun MappingsSection(
    mappings: List<DebugMapping>,
    loaded: Boolean,
    enabled: Boolean,
    onNew: () -> Unit,
    onEdit: (DebugMapping) -> Unit,
    onToggle: (DebugMapping, Boolean) -> Unit,
    onRemove: (DebugMapping) -> Unit,
) {
    Column {
        Section(
            stringResource(R.string.mappings),
            action = { TextButton(onClick = onNew, enabled = enabled) { Text(stringResource(R.string.mapping_new)) } },
        ) {
            if (loaded && mappings.isEmpty()) {
                EmptyRows(stringResource(R.string.mappings_empty), stringResource(R.string.mappings_empty_body))
            }
            mappings.forEachIndexed { index, entry ->
                if (index > 0) RowDivider()
                val label = mappingLabel(entry)
                val hint = if (entry.kind == "local") {
                    stringResource(R.string.mapping_local_hint, entry.status, formatBytes(MappingText.bytes(entry.body).size.toLong()))
                } else {
                    stringResource(R.string.mapping_remote_hint, entry.target)
                }
                ToggleLine(label, hint, entry.enabled, enabled, onToggle = { onToggle(entry, it) }) { close ->
                    MenuItem(R.string.action_edit, close) { onEdit(entry) }
                    MenuItem(R.string.mapping_remove, close) { onRemove(entry) }
                }
            }
        }
        Note(stringResource(R.string.mappings_note))
    }
}

/** The destination rewrites; one opens in a dialog with the editor's fields. */
@Composable
fun RedirectsSection(
    redirects: List<TrafficRedirect>,
    loaded: Boolean,
    enabled: Boolean,
    onNew: () -> Unit,
    onEdit: (TrafficRedirect) -> Unit,
    onToggle: (TrafficRedirect, Boolean) -> Unit,
    onRemove: (TrafficRedirect) -> Unit,
) {
    Column {
        Section(
            stringResource(R.string.redirects),
            action = { TextButton(onClick = onNew, enabled = enabled) { Text(stringResource(R.string.redirect_new)) } },
        ) {
            if (loaded && redirects.isEmpty()) {
                EmptyRows(stringResource(R.string.redirects_empty), stringResource(R.string.redirects_empty_body))
            }
            redirects.forEachIndexed { index, entry ->
                if (index > 0) RowDivider()
                ToggleLine(redirectLabel(entry), "→ " + entry.target, entry.enabled, enabled, onToggle = { onToggle(entry, it) }) { close ->
                    if (entry.draft != null) MenuItem(R.string.action_edit, close) { onEdit(entry) }
                    MenuItem(R.string.redirect_remove, close) { onRemove(entry) }
                }
            }
        }
        Note(stringResource(R.string.redirects_note))
    }
}

@Composable
private fun EmptyRows(title: String, body: String) {
    Column(modifier = Modifier.padding(BrandSpacing.md), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
        Text(title, style = MaterialTheme.typography.titleMedium)
        Text(body, style = MaterialTheme.typography.bodyMedium, color = LocalBrandPalette.current.text2)
    }
}

/** A row with a name, a monospace line under it, a ⋮ menu and a switch. */
@Composable
private fun ToggleLine(
    label: String,
    hint: String,
    checked: Boolean,
    enabled: Boolean,
    onToggle: (Boolean) -> Unit,
    menu: @Composable (close: () -> Unit) -> Unit,
) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = RowMinHeight)
            .padding(start = BrandSpacing.md, end = BrandSpacing.md, top = BrandSpacing.sm, bottom = BrandSpacing.sm),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
            Text(label, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                hint,
                style = MaterialTheme.typography.bodySmall,
                color = LocalBrandPalette.current.text2,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        OverflowMenu(stringResource(R.string.proxies_more, label), enabled, menu)
        Switch(checked = checked, onCheckedChange = onToggle, enabled = enabled)
    }
}

/** A body in a text box: editable when it is text, kept as the saved bytes otherwise until cleared. */
private sealed interface BodyDraft {
    data class Text(val text: String) : BodyDraft

    class Bytes(val bytes: ByteArray) : BodyDraft
}

private fun bodyDraft(base64: String): BodyDraft {
    val bytes = MappingText.bytes(base64)
    return MappingText.text(bytes)?.let { BodyDraft.Text(it) } ?: BodyDraft.Bytes(bytes)
}

/** Adds or changes one mapping; the page replaces 扩展 until it is saved or left. */
@Composable
fun MappingEditorScreen(
    initial: DebugMapping,
    busy: Boolean,
    onSave: (DebugMapping) -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var entry by remember(initial) { mutableStateOf(initial) }
    var headers by rememberSaveable(initial) { mutableStateOf(MappingText.headerLines(initial.headers)) }
    var body by remember(initial) { mutableStateOf(bodyDraft(initial.body)) }
    var status by rememberSaveable(initial) { mutableStateOf(initial.status.toString()) }
    val local = entry.kind == "local"
    val statusCode = status.toIntOrNull()?.takeIf { it in STATUS_RANGE }
    val canSave = entry.pattern.isNotBlank() && (if (local) statusCode != null else entry.target.isNotBlank())
    Page(
        stringResource(if (initial.id.isEmpty()) R.string.mapping_new else R.string.mapping_edit),
        modifier,
        onBack = onBack,
    ) {
        Text(
            stringResource(R.string.mapping_editor_note),
            style = MaterialTheme.typography.bodyMedium,
            color = LocalBrandPalette.current.text2,
        )
        Choice(
            stringResource(R.string.mapping_kind),
            listOf("local" to stringResource(R.string.mapping_kind_local), "remote" to stringResource(R.string.mapping_kind_remote)),
            entry.kind,
        ) { entry = entry.copy(kind = it) }
        Choice(
            stringResource(R.string.mapping_match),
            listOf(
                "url" to stringResource(R.string.mapping_match_url),
                "prefix" to stringResource(R.string.mapping_match_prefix),
                "regex" to stringResource(R.string.mapping_match_regex),
            ),
            entry.match,
            hint = stringResource(
                when (entry.match) {
                    "prefix" -> R.string.mapping_match_prefix_hint
                    "regex" -> R.string.mapping_match_regex_hint
                    else -> R.string.mapping_match_url_hint
                },
            ),
        ) { entry = entry.copy(match = it) }
        Field(
            stringResource(if (entry.match == "regex") R.string.mapping_regex else R.string.mapping_url),
            entry.pattern,
            placeholder = "https://",
            keyboard = KeyboardType.Uri,
        ) { entry = entry.copy(pattern = it) }
        Field(stringResource(R.string.mapping_name), entry.name) { entry = entry.copy(name = it) }
        if (local) {
            Field(stringResource(R.string.mapping_status), status, keyboard = KeyboardType.Number, error = statusCode == null) { status = it }
            Field(stringResource(R.string.mapping_headers), headers, monospace = true, lines = 3) { headers = it }
            when (val draft = body) {
                is BodyDraft.Text -> Field(stringResource(R.string.mapping_body), draft.text, monospace = true, lines = 6) {
                    body = BodyDraft.Text(it)
                }
                is BodyDraft.Bytes -> Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
                ) {
                    Text(
                        stringResource(R.string.mapping_body_binary, formatBytes(draft.bytes.size.toLong())),
                        style = MaterialTheme.typography.bodyMedium,
                        modifier = Modifier.weight(1f),
                    )
                    OutlinedButton(onClick = { body = BodyDraft.Text("") }) { Text(stringResource(R.string.action_clear)) }
                }
            }
        } else {
            Field(
                stringResource(if (entry.match == "regex") R.string.mapping_target_regex else R.string.mapping_target),
                entry.target,
                placeholder = "https://",
                keyboard = KeyboardType.Uri,
            ) { entry = entry.copy(target = it) }
        }
        Row(horizontalArrangement = Arrangement.spacedBy(BrandSpacing.sm)) {
            Button(
                onClick = {
                    onSave(
                        entry.copy(
                            pattern = entry.pattern.trim(),
                            status = statusCode ?: entry.status,
                            headers = if (local) MappingText.parseHeaderLines(headers) else emptyList(),
                            body = when (val draft = body) {
                                is BodyDraft.Text -> if (local) MappingText.base64(draft.text) else ""
                                is BodyDraft.Bytes -> if (local) MappingText.base64(draft.bytes) else ""
                            },
                            target = if (local) "" else entry.target.trim(),
                        ),
                    )
                },
                enabled = !busy && canSave,
            ) { Text(stringResource(R.string.action_save)) }
            OutlinedButton(onClick = onBack) { Text(stringResource(R.string.action_cancel)) }
        }
    }
}

private val STATUS_RANGE = 100..599

/** A labelled choice of a few values as segmented buttons, with an optional sentence under it. */
@Composable
private fun Choice(label: String, options: List<Pair<String, String>>, selected: String, hint: String? = null, onChoose: (String) -> Unit) {
    Column(verticalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
        Text(label, style = MaterialTheme.typography.labelMedium, color = LocalBrandPalette.current.text2)
        SingleChoiceSegmentedButtonRow(modifier = Modifier.fillMaxWidth()) {
            options.forEachIndexed { index, (value, text) ->
                SegmentedButton(
                    selected = value == selected,
                    onClick = { onChoose(value) },
                    shape = SegmentedButtonDefaults.itemShape(index, options.size),
                ) { Text(text, maxLines = 1, overflow = TextOverflow.Ellipsis) }
            }
        }
        if (hint != null) Text(hint, style = MaterialTheme.typography.bodySmall, color = LocalBrandPalette.current.text2)
    }
}

@Composable
private fun Field(
    label: String,
    value: String,
    placeholder: String? = null,
    keyboard: KeyboardType = KeyboardType.Text,
    monospace: Boolean = false,
    lines: Int = 1,
    error: Boolean = false,
    onChange: (String) -> Unit,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onChange,
        label = { Text(label) },
        placeholder = placeholder?.let { { Text(it) } },
        singleLine = lines == 1,
        minLines = lines,
        isError = error,
        keyboardOptions = KeyboardOptions(keyboardType = keyboard, autoCorrectEnabled = false),
        textStyle = if (monospace) {
            MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace)
        } else {
            MaterialTheme.typography.bodyLarge
        },
        modifier = Modifier.fillMaxWidth(),
    )
}

/** Adds or changes one destination rewrite. */
@Composable
fun RedirectDialog(editor: RedirectEditor, busy: Boolean, onSave: (RedirectDraft) -> Unit, onDismiss: () -> Unit) {
    var source by rememberSaveable(editor) { mutableStateOf(editor.draft.source) }
    var ports by rememberSaveable(editor) { mutableStateOf(editor.draft.ports) }
    var network by rememberSaveable(editor) { mutableStateOf(editor.draft.network) }
    var target by rememberSaveable(editor) { mutableStateOf(editor.draft.target) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(if (editor.id == null) R.string.redirect_new else R.string.redirect_edit)) },
        text = {
            Column(
                modifier = Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
            ) {
                Text(stringResource(R.string.redirects_note), style = MaterialTheme.typography.bodySmall, color = LocalBrandPalette.current.text2)
                Field(stringResource(R.string.redirect_source), source, placeholder = "203.0.113.10", keyboard = KeyboardType.Uri) { source = it }
                Field(stringResource(R.string.redirect_ports), ports, placeholder = "7000-7100", keyboard = KeyboardType.Uri) { ports = it }
                Choice(
                    stringResource(R.string.redirect_network),
                    listOf("any" to stringResource(R.string.redirect_network_any), "tcp" to "TCP", "udp" to "UDP"),
                    network,
                ) { network = it }
                Field(stringResource(R.string.redirect_target), target, placeholder = "192.168.1.20:7001", keyboard = KeyboardType.Uri) {
                    target = it
                }
            }
        },
        confirmButton = {
            TextButton(
                onClick = { onSave(RedirectDraft(source.trim(), ports.trim(), network, target.trim())) },
                enabled = !busy && target.isNotBlank(),
            ) { Text(stringResource(R.string.action_save)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

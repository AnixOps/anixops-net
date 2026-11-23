package io.anixops.net.ui

import androidx.annotation.StringRes
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Share
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.unit.dp
import io.anixops.net.R
import io.anixops.net.core.CaptureMessage
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/**
 * One request opened from the list: what it was, a way to send it again, its headers and bodies (the request's
 * before the response's), and its export.
 */
@Composable
fun RequestDetailScreen(opened: OpenedRequest, requests: RequestsState, actions: RequestsActions, modifier: Modifier = Modifier) {
    val exchange = opened.exchange
    Page(stringResource(R.string.request_detail), modifier, actions.close) {
        SummarySection(opened)
        when {
            exchange != null -> {
                Section(title = null) {
                    InfoRow(
                        stringResource(R.string.replay),
                        stringResource(R.string.replay_edit),
                        onClick = actions.openReplay,
                    )
                    InfoRow(
                        stringResource(R.string.request_connection),
                        stringResource(R.string.request_connection_open),
                        divider = true,
                        onClick = { actions.openConnectionOf(opened.row) },
                    )
                }
                MessageSections(R.string.request_headers, R.string.request_body, exchange.request, opened.requestBody)
                MessageSections(R.string.response_headers, R.string.response_body, exchange.response, opened.responseBody)
            }
            opened.gone -> Note(stringResource(R.string.request_gone))
            else -> Note(stringResource(R.string.request_loading))
        }
        if (!opened.gone) ExportSection(requests, actions)
    }
}

/** Saves this one request as a command, code, or a collection for an API tool, in Downloads. */
@Composable
private fun ExportSection(requests: RequestsState, actions: RequestsActions) {
    Column {
        Section(stringResource(R.string.export_request)) {
            var choosing by remember { mutableStateOf(false) }
            Box {
                InfoRow(
                    stringResource(R.string.export_format),
                    ExportFormats.firstOrNull { it.first == requests.exportFormat }?.second.orEmpty(),
                    onClick = { choosing = true },
                )
                DropdownMenu(expanded = choosing, onDismissRequest = { choosing = false }) {
                    ExportFormats.forEach { (id, label) ->
                        DropdownMenuItem(
                            text = { Text(label) },
                            onClick = {
                                choosing = false
                                actions.setExportFormat(id)
                            },
                        )
                    }
                }
            }
            RowDivider()
            SwitchRow(
                stringResource(R.string.export_redact),
                stringResource(R.string.export_redact_hint),
                checked = requests.redact,
                enabled = true,
                onChange = actions.setRedact,
            )
            RowDivider()
            Row(Modifier.padding(BrandSpacing.md)) {
                Button(onClick = actions.export) { Text(stringResource(R.string.export_save)) }
            }
        }
        Note(stringResource(R.string.export_note))
    }
}

@Composable
private fun SummarySection(opened: OpenedRequest) {
    val row = opened.row
    val palette = LocalBrandPalette.current
    val locale = LocalConfiguration.current.locales[0]
    Section(stringResource(R.string.request_summary)) {
        Column(
            modifier = Modifier
                .fillMaxWidth()
                .padding(BrandSpacing.md),
            verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
                Text(row.method, style = MaterialTheme.typography.titleMedium)
                Text(
                    statusText(row),
                    style = MaterialTheme.typography.titleMedium,
                    fontFamily = FontFamily.Monospace,
                    color = toneColor(row.tone),
                )
            }
            SelectionContainer {
                Text(row.url, style = MaterialTheme.typography.bodyMedium, color = palette.text2)
            }
        }
        InfoRow(stringResource(R.string.request_version), row.httpVersion, divider = true)
        InfoRow(
            stringResource(R.string.request_started),
            DateFormat.getTimeInstance(DateFormat.MEDIUM, locale).format(Date(row.startedAtMs)),
            divider = true,
        )
        InfoRow(
            stringResource(R.string.request_duration),
            row.durationMs?.let(::formatDuration) ?: stringResource(R.string.request_in_progress),
            divider = true,
        )
        InfoRow(stringResource(R.string.request_sizes), "${formatBytes(row.requestSize)} / ${formatBytes(row.responseSize)}", divider = true)
        noteText(row.note)?.let { InfoRow(stringResource(R.string.request_note), it, divider = true) }
        if (opened.exchange?.rulesSkipped == true) Note(stringResource(R.string.request_rules_skipped))
    }
}

@Composable
private fun noteText(note: String?): String? = when (note) {
    "answered_locally" -> stringResource(R.string.request_local)
    "streaming" -> stringResource(R.string.request_streaming)
    "too_large" -> stringResource(R.string.request_too_large)
    else -> null
}

@Composable
internal fun MessageSections(@StringRes headersTitle: Int, @StringRes bodyTitle: Int, message: CaptureMessage, body: BodyText?) {
    val palette = LocalBrandPalette.current
    Section(stringResource(headersTitle)) {
        if (message.headers.isEmpty()) {
            Text(
                stringResource(R.string.request_no_headers),
                style = MaterialTheme.typography.bodyMedium,
                color = palette.text2,
                modifier = Modifier.padding(BrandSpacing.md),
            )
        }
        SelectionContainer {
            Column {
                message.headers.filter { it.size == 2 }.forEachIndexed { index, (name, value) ->
                    if (index > 0) RowDivider()
                    Column(
                        modifier = Modifier
                            .fillMaxWidth()
                            .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.xs),
                    ) {
                        Text(name, style = MaterialTheme.typography.labelMedium, color = palette.text2)
                        Text(value, style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace)
                    }
                }
            }
        }
    }
    if (body != null) BodySection(stringResource(bodyTitle), body, if (bodyTitle == R.string.request_body) "request-body" else "response-body")
}

/**
 * A body, shown by what it is: text (JSON indented), form fields, a picture, Protobuf decoded without a schema, or
 * hex; the view can be switched, and hex is always one of them. Under it, the body as a file: open it in another app
 * (only types that run no code there) or share it. [fileName] names the file, without its extension.
 */
@Composable
internal fun BodySection(title: String, body: BodyText, fileName: String) {
    val palette = LocalBrandPalette.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val modes = body.modes
    var picked by rememberSaveable(body) { mutableStateOf<BodyMode?>(null) }
    val mode = picked?.takeIf { it in modes } ?: modes.first()
    Column {
        Section("$title · ${formatBytes(body.size)}") {
            if (body.size == 0L) {
                Text(
                    stringResource(R.string.request_no_body),
                    style = MaterialTheme.typography.bodyMedium,
                    color = palette.text2,
                    modifier = Modifier.padding(BrandSpacing.md),
                )
            } else {
                BodyViews(body, modes, mode, onPick = { picked = it })
                if (body.bytes.isNotEmpty()) {
                    RowDivider()
                    Row(
                        modifier = Modifier.fillMaxWidth().padding(horizontal = BrandSpacing.xs),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        if (CaptureBinary.fileType(body.contentType).second) {
                            TextButton(onClick = { scope.launch { BodyFiles.open(context, body.bytes, body.contentType, fileName) } }) {
                                Text(stringResource(R.string.body_open))
                            }
                        }
                        Spacer(Modifier.weight(1f))
                        IconButton(onClick = { scope.launch { BodyFiles.share(context, body.bytes, body.contentType, fileName) } }) {
                            Icon(Icons.Filled.Share, contentDescription = stringResource(R.string.body_share))
                        }
                    }
                }
            }
        }
        if (body.truncated) Note(stringResource(R.string.request_body_truncated))
        if (body.undecoded) Note(stringResource(R.string.request_body_undecoded))
    }
}

/** The view switch (when there is more than one view) and the body in the chosen one. */
@Composable
private fun BodyViews(body: BodyText, modes: List<BodyMode>, mode: BodyMode, onPick: (BodyMode) -> Unit) {
    if (modes.size > 1) {
        SingleChoiceSegmentedButtonRow(modifier = Modifier.fillMaxWidth().padding(BrandSpacing.md)) {
            modes.forEachIndexed { index, option ->
                SegmentedButton(
                    selected = option == mode,
                    onClick = { onPick(option) },
                    shape = SegmentedButtonDefaults.itemShape(index, modes.size),
                ) {
                    Text(modeTitle(option), maxLines = 1)
                }
            }
        }
        RowDivider()
    }
    BodyContent(body, mode)
}

@Composable
private fun modeTitle(mode: BodyMode): String = when (mode) {
    BodyMode.Raw -> stringResource(R.string.body_mode_raw)
    BodyMode.Table -> stringResource(R.string.body_mode_table)
    BodyMode.Preview -> stringResource(R.string.body_mode_preview)
    BodyMode.Protobuf -> "Protobuf"
    BodyMode.Hex -> stringResource(R.string.body_mode_hex)
}

/** The body in the chosen view. Hex and decoded Protobuf are worked out once per body, the picture off the main thread. */
@Composable
private fun BodyContent(body: BodyText, mode: BodyMode) {
    val resources = LocalContext.current.resources
    when (mode) {
        BodyMode.Raw -> Code(body.text.orEmpty())
        BodyMode.Table -> {
            val pairs = (body.kind as? BodyKind.Form)?.pairs.orEmpty()
            SelectionContainer {
                Column {
                    pairs.forEachIndexed { index, (name, value) ->
                        if (index > 0) RowDivider()
                        Column(Modifier.fillMaxWidth().padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.xs)) {
                            Text(name, style = MaterialTheme.typography.labelMedium, color = LocalBrandPalette.current.text2)
                            Text(value, style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace)
                        }
                    }
                }
            }
        }
        BodyMode.Protobuf -> {
            val text = remember(body) {
                val kind = body.kind as? BodyKind.Protobuf
                val more = resources.getString(R.string.proto_more)
                val labels = CaptureBinary.Labels(
                    message = { index, size -> resources.getString(R.string.proto_message, index, size) },
                    compressed = resources.getString(R.string.proto_compressed),
                    trailers = resources.getString(R.string.proto_trailers),
                    undecoded = resources.getString(R.string.proto_undecoded),
                    more = more,
                )
                when {
                    kind?.grpc != null -> CaptureBinary.grpcText(kind.grpc, labels)
                    kind?.message != null -> CaptureBinary.protoText(kind.message, more)
                    else -> CaptureBinary.decodeProtobuf(body.bytes, body.truncated)?.let { CaptureBinary.protoText(it, more) }.orEmpty()
                }
            }
            Code(text)
        }
        BodyMode.Preview -> {
            val picture by produceState<BodyFiles.Picture?>(null, body) {
                value = withContext(Dispatchers.Default) { BodyFiles.picture(body.bytes) }
            }
            when (val shown = picture) {
                null -> Unit
                is BodyFiles.Picture.Drawn -> Image(
                    bitmap = shown.bitmap.asImageBitmap(),
                    contentDescription = stringResource(R.string.body_image_preview),
                    contentScale = ContentScale.Fit,
                    modifier = Modifier.fillMaxWidth().heightIn(max = 480.dp).padding(BrandSpacing.md),
                )
                is BodyFiles.Picture.TooLarge -> Hex(body, stringResource(R.string.body_image_too_large, shown.width, shown.height))
                BodyFiles.Picture.Unreadable -> Hex(body, stringResource(R.string.body_image_unreadable))
            }
        }
        BodyMode.Hex -> Hex(body, null)
    }
}

@Composable
private fun Hex(body: BodyText, note: String?) {
    val dump = remember(body) { CaptureBytes.hexDump(body.bytes) }
    Column {
        if (note != null) {
            Text(
                note,
                style = MaterialTheme.typography.bodySmall,
                color = LocalBrandPalette.current.text2,
                modifier = Modifier.padding(start = BrandSpacing.md, end = BrandSpacing.md, top = BrandSpacing.md),
            )
        }
        Code(dump)
    }
}

/** Monospaced, selectable text; very long text is cut so the page stays responsive. */
@Composable
private fun Code(text: String) {
    val shown = if (text.length > BodyText.MAX_CHARS) text.take(BodyText.MAX_CHARS) + "\n…" else text
    SelectionContainer {
        Text(
            shown,
            style = MaterialTheme.typography.bodySmall,
            fontFamily = FontFamily.Monospace,
            modifier = Modifier.padding(BrandSpacing.md),
        )
    }
}

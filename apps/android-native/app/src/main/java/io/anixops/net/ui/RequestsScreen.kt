package io.anixops.net.ui

import androidx.annotation.StringRes
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CornerSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.FilterChip
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.anixops.net.R
import io.anixops.net.core.CaptureSession
import io.anixops.net.core.CaptureStatus
import io.anixops.net.core.ConnectionSnapshot
import io.anixops.net.core.FilterError
import io.anixops.net.core.RequestRow
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette
import java.util.Locale

/** What the requests page asks of its view models. */
data class RequestsActions(
    val setCapture: (Boolean) -> Unit = {},
    val clear: () -> Unit = {},
    val toggleQuick: (String) -> Unit = {},
    val setTyped: (String) -> Unit = {},
    val setOrder: (String) -> Unit = {},
    val setPaused: (Boolean) -> Unit = {},
    val open: (RequestRow) -> Unit = {},
    val close: () -> Unit = {},
    val setExportFormat: (String) -> Unit = {},
    val setRedact: (Boolean) -> Unit = {},
    val export: () -> Unit = {},
    val openReplay: () -> Unit = {},
    val editReplay: ((ReplayDraft) -> ReplayDraft) -> Unit = {},
    val sendReplay: () -> Unit = {},
    val closeReplay: () -> Unit = {},
    /** Exports everything: the format, and whether secrets are masked. */
    val exportAll: (String, Boolean) -> Unit = { _, _ -> },
    val openSessions: () -> Unit = {},
    val closeSessions: () -> Unit = {},
    val setSessionName: (String) -> Unit = {},
    val saveSession: () -> Unit = {},
    val loadSession: (CaptureSession) -> Unit = {},
    val deleteSession: (CaptureSession) -> Unit = {},
    /** Goes to 改写, where decryption and its domains are turned on. */
    val openRewrite: () -> Unit = {},
    val setShowConnections: (Boolean) -> Unit = {},
    val openConnection: (Long, String) -> Unit = { _, _ -> },
    val closeConnection: () -> Unit = {},
    val openExchange: (Int) -> Unit = {},
    val openConnectionOf: (RequestRow) -> Unit = {},
    val decryptConnectionHost: () -> Unit = {},
    val exportConnection: (String) -> Unit = {},
)

/** Width of a checklist step's number. */
private val StepNumberWidth = 20.dp

/** Width of the status column: a code, "…", or the short word for a request answered on the device. */
private val StatusWidth = 48.dp

/**
 * 调试 › 请求 (docs/debug-mode.md): the connection and capture, the filters, then every decrypted request, newest
 * first; a tap opens it. The list can hold thousands of rows, so it is a lazy list rather than a [Page].
 */
@Composable
fun RequestsScreen(state: UiState, requests: RequestsState, actions: RequestsActions, modifier: Modifier = Modifier) {
    requests.sessions?.let { page ->
        SessionsScreen(requests.capture, page, actions, modifier)
        return
    }
    requests.replay?.let { draft ->
        ReplayScreen(draft, actions, modifier)
        return
    }
    requests.opened?.let { opened ->
        RequestDetailScreen(opened, requests, actions, modifier)
        return
    }
    requests.connection?.let { view ->
        ConnectionScreen(view, requests, actions, modifier)
        return
    }
    val capture = requests.capture
    val holding = capture != null && (capture.enabled || capture.connections > 0)
    // Disconnected, capture is the session saved at the disconnect (or one opened from the saved sessions), held by
    // the capture viewer: it is listed and exported, but nothing is captured and the connection list is gone.
    val connected = state.overview?.state == "connected"
    val saved = holding && !connected
    LazyColumn(
        modifier = modifier
            .fillMaxSize()
            .windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Top + WindowInsetsSides.Horizontal)),
        horizontalAlignment = Alignment.CenterHorizontally,
        contentPadding = PaddingValues(vertical = BrandSpacing.lg),
    ) {
        item {
            Column(Modifier.content(), verticalArrangement = Arrangement.spacedBy(BrandSpacing.lg)) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        stringResource(R.string.nav_requests),
                        style = MaterialTheme.typography.headlineSmall,
                        modifier = Modifier.weight(1f),
                    )
                    if (capture != null) RequestsMenu(requests, saved, actions)
                }
                if (capture != null && connected) CaptureSection(capture, requests.busy, actions)
                if (saved) {
                    Text(
                        stringResource(R.string.requests_saved_hint),
                        style = MaterialTheme.typography.bodyMedium,
                        color = LocalBrandPalette.current.text2,
                    )
                }
                if (requests.setup.isNotEmpty()) SetupSection(requests.setup, actions)
                if (holding) {
                    if (connected) Row(horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
                        FilterChip(
                            selected = !requests.showConnections,
                            onClick = { actions.setShowConnections(false) },
                            label = { Text(stringResource(R.string.requests_tab_requests)) },
                        )
                        FilterChip(
                            selected = requests.showConnections,
                            onClick = { actions.setShowConnections(true) },
                            label = { Text(stringResource(R.string.requests_tab_connections)) },
                        )
                    }
                    if (!requests.showConnections || !connected) FilterBar(requests, actions)
                }
            }
        }
        if (holding && connected && requests.showConnections) {
            // Newest first, as the requests.
            val connections = state.connections.asReversed()
            if (connections.isEmpty()) item { Hint(stringResource(R.string.connections_empty)) }
            itemsIndexed(connections, key = { _, connection -> connection.id }) { index, connection ->
                ConnectionLine(connection, first = index == 0, last = index == connections.lastIndex) {
                    actions.openConnection(connection.id, connection.name)
                }
            }
        }
        if (holding && (!requests.showConnections || !connected)) {
            if (requests.paused) item { Hint(stringResource(R.string.requests_paused)) }
            if (requests.loaded && requests.rows.isEmpty()) {
                val filtered = requests.quick.isNotEmpty() || requests.typed.isNotBlank()
                item { Hint(stringResource(if (filtered) R.string.requests_no_match else R.string.requests_empty)) }
            }
            itemsIndexed(requests.rows, key = { _, row -> "${row.connectionId}:${row.index}" }) { index, row ->
                RequestLine(row, first = index == 0, last = index == requests.rows.lastIndex) { actions.open(row) }
            }
        }
    }
}

/** A line of a list drawn as one card: the first rounds its top corners, the last its bottom ones. */
@Composable
internal fun lineShape(first: Boolean, last: Boolean): RoundedCornerShape {
    val corner = MaterialTheme.shapes.medium.topStart
    val none = CornerSize(0.dp)
    return RoundedCornerShape(
        topStart = if (first) corner else none,
        topEnd = if (first) corner else none,
        bottomStart = if (last) corner else none,
        bottomEnd = if (last) corner else none,
    )
}

/** One open connection: its name, and how it goes and how much it moved; a tap opens what capture holds of it. */
@Composable
private fun ConnectionLine(connection: ConnectionSnapshot, first: Boolean, last: Boolean, onOpen: () -> Unit) {
    val palette = LocalBrandPalette.current
    Surface(shape = lineShape(first, last), color = palette.surface, contentColor = palette.text, modifier = Modifier.content()) {
        Column {
            if (!first) RowDivider()
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable(onClick = onOpen)
                    .heightIn(min = RowMinHeight)
                    .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.xs),
                verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs),
            ) {
                Text(connection.name, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(
                    listOf(connection.network.uppercase(Locale.ROOT), connection.via, formatBytes(connection.upload + connection.download))
                        .filter { it.isNotBlank() }
                        .joinToString(" · "),
                    style = MaterialTheme.typography.bodySmall,
                    color = palette.text2,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

/** A lazy item's width: the page's column, centred on tablets. */
internal fun Modifier.content(): Modifier = widthIn(max = ContentMaxWidth)
    .fillMaxWidth()
    .padding(horizontal = BrandSpacing.md)

@Composable
private fun Hint(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.bodyMedium,
        color = LocalBrandPalette.current.text2,
        modifier = Modifier
            .content()
            .padding(start = BrandSpacing.md, top = BrandSpacing.xs, bottom = BrandSpacing.sm),
    )
}

/**
 * Capture on or off. The connection switch is not here: debug mode keeps it in the bar under every page
 * ([DebugConnectionBar]), so it is also there on 改写, 出口, 设置 and the pages opened from this one.
 */
@Composable
private fun CaptureSection(capture: CaptureStatus, busy: Boolean, actions: RequestsActions) {
    Section(title = null) {
        SwitchRow(
            stringResource(R.string.requests_capture),
            pluralStringResource(
                R.plurals.requests_capture_usage,
                capture.connections,
                capture.connections,
                formatBytes(capture.usedBytes),
                formatBytes(capture.maxTotalBytes),
            ),
            checked = capture.enabled,
            enabled = !busy,
            onChange = actions.setCapture,
        )
    }
}

/**
 * What is left before HTTPS requests show (docs/debug-mode.md), in the order it has to happen, each saying where it
 * is done; it goes once nothing is left. The switches above do the first and last steps; 改写 does the others.
 */
@Composable
private fun SetupSection(steps: List<String>, actions: RequestsActions) {
    Section(stringResource(R.string.setup_title)) {
        steps.forEachIndexed { index, step ->
            val (title, hint) = when (step) {
                "connect" -> R.string.setup_connect to R.string.setup_connect_hint
                "certificate" -> R.string.setup_certificate to R.string.setup_certificate_hint
                "decryption" -> R.string.setup_decryption to R.string.setup_decryption_hint
                "domains" -> R.string.setup_domains to R.string.setup_domains_hint
                else -> R.string.setup_capture to R.string.setup_capture_hint
            }
            if (index > 0) RowDivider()
            SetupStep(index + 1, title, hint, inRewrite = step == "certificate" || step == "decryption" || step == "domains", actions)
        }
    }
}

@Composable
private fun SetupStep(number: Int, @StringRes title: Int, @StringRes hint: Int, inRewrite: Boolean, actions: RequestsActions) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = RowMinHeight)
            .padding(start = BrandSpacing.md, end = BrandSpacing.xs, top = BrandSpacing.sm, bottom = BrandSpacing.sm),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
    ) {
        Text(
            number.toString(),
            style = MaterialTheme.typography.titleMedium,
            color = MaterialTheme.colorScheme.primary,
            modifier = Modifier.width(StepNumberWidth),
        )
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
            Text(stringResource(title), style = MaterialTheme.typography.bodyLarge)
            Text(stringResource(hint), style = MaterialTheme.typography.bodySmall, color = LocalBrandPalette.current.text2)
        }
        if (inRewrite) TextButton(onClick = actions.openRewrite) { Text(stringResource(R.string.setup_go)) }
    }
}

@Composable
private fun RequestsMenu(requests: RequestsState, saved: Boolean, actions: RequestsActions) {
    OverflowMenu(stringResource(R.string.requests_more), enabled = !requests.busy) { close ->
        if (requests.paused) {
            MenuItem(R.string.requests_resume, close) { actions.setPaused(false) }
        } else {
            MenuItem(R.string.requests_pause, close) { actions.setPaused(true) }
        }
        HorizontalDivider()
        RequestOrders.forEach { order ->
            val chosen = order == requests.order
            DropdownMenuItem(
                text = {
                    Text(
                        stringResource(orderLabel(order)) +
                            if (chosen) " " + stringResource(if (requests.descending) R.string.order_descending else R.string.order_ascending) else "",
                    )
                },
                onClick = {
                    close()
                    actions.setOrder(order)
                },
            )
        }
        HorizontalDivider()
        val holding = (requests.capture?.connections ?: 0) > 0
        // One zip with the connection list, HAR and pcapng, masked or complete; then the HAR and pcapng alone.
        listOf(
            Triple(R.string.requests_export_zip, "zip", true),
            Triple(R.string.requests_export_zip_full, "zip", false),
            Triple(R.string.requests_export_har, "har", true),
            Triple(R.string.requests_export_pcapng, "pcapng", true),
        ).forEach { (label, format, redact) ->
            DropdownMenuItem(
                text = { Text(stringResource(label)) },
                enabled = holding,
                onClick = {
                    close()
                    actions.exportAll(format, redact)
                },
            )
        }
        MenuItem(R.string.sessions, close, actions.openSessions)
        HorizontalDivider()
        MenuItem(if (saved) R.string.requests_close_session else R.string.requests_clear, close, actions.clear)
    }
}

@StringRes
private fun orderLabel(order: String): Int = when (order) {
    "status" -> R.string.order_status
    "host" -> R.string.order_host
    "size" -> R.string.order_size
    "duration" -> R.string.order_duration
    else -> R.string.order_time
}

@StringRes
private fun quickLabel(id: String): Int = when (id) {
    "errors" -> R.string.quick_errors
    "json" -> R.string.quick_json
    "images" -> R.string.quick_images
    "h2" -> R.string.quick_h2
    else -> R.string.quick_large
}

@Composable
private fun FilterBar(requests: RequestsState, actions: RequestsActions) {
    Column(verticalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
        val error = requests.error
        OutlinedTextField(
            value = requests.typed,
            onValueChange = actions.setTyped,
            singleLine = true,
            isError = error != null,
            leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null) },
            placeholder = { Text(stringResource(R.string.requests_filter), maxLines = 1, overflow = TextOverflow.Ellipsis) },
            supportingText = { Text(error?.let { filterError(it) } ?: stringResource(R.string.requests_filter_hint)) },
            modifier = Modifier.fillMaxWidth(),
        )
        Row(
            horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs),
            modifier = Modifier.horizontalScroll(rememberScrollState()),
        ) {
            QuickFilters.forEach { id ->
                FilterChip(
                    selected = id in requests.quick,
                    onClick = { actions.toggleQuick(id) },
                    label = { Text(stringResource(quickLabel(id))) },
                )
            }
        }
        Text(
            pluralStringResource(R.plurals.requests_count, requests.rows.size, requests.rows.size),
            style = MaterialTheme.typography.labelMedium,
            color = LocalBrandPalette.current.text2,
            modifier = Modifier.padding(start = BrandSpacing.md),
        )
    }
}

@Composable
private fun filterError(error: FilterError): String {
    val at = error.at
    return when {
        error.code == "invalid_filter" && at != null -> stringResource(R.string.requests_filter_error_at, at + 1)
        error.code == "invalid_filter" -> stringResource(R.string.requests_filter_error)
        else -> error.message ?: error.code
    }
}

/** The colour of a status [tone] from `view.request_rows`. */
@Composable
internal fun toneColor(tone: String): Color {
    val palette = LocalBrandPalette.current
    return when (tone) {
        "success" -> palette.success
        "warning" -> palette.warning
        "danger" -> palette.danger
        else -> palette.text2
    }
}

/** What the status column says: the code, … while the response is on its way, or — when there is none. */
@Composable
internal fun statusText(row: RequestRow): String = when {
    row.status != null -> "${row.status}"
    row.tone == "pending" -> "…"
    row.note == "answered_locally" -> stringResource(R.string.request_local)
    else -> "—"
}

/** One request on the grouped card the rows make together: rounded at the top of the first and the foot of the last. */
@Composable
private fun RequestLine(row: RequestRow, first: Boolean, last: Boolean, onOpen: () -> Unit) {
    val palette = LocalBrandPalette.current
    Surface(
        shape = lineShape(first, last),
        color = palette.surface,
        contentColor = palette.text,
        modifier = Modifier.content(),
    ) {
        Column {
            if (!first) RowDivider()
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .clickable(onClick = onOpen)
                    .heightIn(min = RowMinHeight)
                    .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.xs),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
            ) {
                Text(
                    if (row.status == null && row.note == "answered_locally") stringResource(R.string.request_local_short) else statusText(row),
                    style = MaterialTheme.typography.labelLarge,
                    // Codes line up in a fixed-width face; the word for a local answer reads better in the text face.
                    fontFamily = if (row.status != null) FontFamily.Monospace else null,
                    color = toneColor(row.tone),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.width(StatusWidth),
                )
                Column(Modifier.weight(1f)) {
                    Text(
                        "${row.method} ${row.host}",
                        style = MaterialTheme.typography.bodyMedium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    Text(
                        row.path,
                        style = MaterialTheme.typography.bodySmall,
                        color = palette.text2,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                Column(horizontalAlignment = Alignment.End) {
                    Text(
                        formatBytes(row.responseSize),
                        style = MaterialTheme.typography.bodySmall,
                        color = palette.text2,
                        textAlign = TextAlign.End,
                    )
                    row.durationMs?.let {
                        Text(
                            formatDuration(it),
                            style = MaterialTheme.typography.bodySmall,
                            color = palette.text2,
                            textAlign = TextAlign.End,
                        )
                    }
                }
            }
        }
    }
}

/** Milliseconds under a second, then seconds with one decimal. */
internal fun formatDuration(millis: Long): String =
    if (millis < 1_000) "$millis ms" else String.format(java.util.Locale.ROOT, "%.1f s", millis / 1_000.0)

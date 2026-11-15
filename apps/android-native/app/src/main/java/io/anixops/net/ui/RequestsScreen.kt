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
import io.anixops.net.core.CaptureStatus
import io.anixops.net.core.FilterError
import io.anixops.net.core.RequestRow
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette

/** What the requests page asks of its view models. */
data class RequestsActions(
    val connect: () -> Unit = {},
    val disconnect: () -> Unit = {},
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
)

/** Width of the status column: a code, "…", or the short word for a request answered on the device. */
private val StatusWidth = 48.dp

/**
 * 调试 › 请求 (docs/debug-mode.md): the connection and capture, the filters, then every decrypted request, newest
 * first; a tap opens it. The list can hold thousands of rows, so it is a lazy list rather than a [Page].
 */
@Composable
fun RequestsScreen(state: UiState, requests: RequestsState, actions: RequestsActions, modifier: Modifier = Modifier) {
    requests.replay?.let { draft ->
        ReplayScreen(draft, actions, modifier)
        return
    }
    requests.opened?.let { opened ->
        RequestDetailScreen(opened, requests, actions, modifier)
        return
    }
    val capture = requests.capture
    val holding = capture != null && (capture.enabled || capture.connections > 0)
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
                    if (capture != null) RequestsMenu(requests, actions)
                }
                ConnectionSection(state, capture, requests.busy, actions)
                when {
                    capture == null -> Empty(R.string.requests_not_connected, R.string.requests_not_connected_body)
                    !holding -> Empty(R.string.requests_capture_off, R.string.requests_capture_off_body)
                    else -> FilterBar(requests, actions)
                }
            }
        }
        if (holding) {
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

/** A lazy item's width: the page's column, centred on tablets. */
private fun Modifier.content(): Modifier = widthIn(max = ContentMaxWidth)
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

/** Debug mode has no 首页, so the connection switch lives here, above capture. */
@Composable
private fun ConnectionSection(state: UiState, capture: CaptureStatus?, busy: Boolean, actions: RequestsActions) {
    val overview = state.overview
    val connected = overview?.state == "connected"
    Section(title = null) {
        SwitchRow(
            stringResource(R.string.home_connection),
            if (state.busy) stringResource(R.string.status_working) else statusLabel(overview?.state ?: "disconnected"),
            checked = connected,
            enabled = !state.busy && overview != null && (connected || overview.canConnect),
            onChange = { on -> if (on) actions.connect() else actions.disconnect() },
        )
        if (capture != null) {
            RowDivider()
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
}

@Composable
private fun Empty(@StringRes title: Int, @StringRes body: Int) {
    Column(verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs), modifier = Modifier.padding(horizontal = BrandSpacing.md)) {
        Text(stringResource(title), style = MaterialTheme.typography.titleMedium)
        Text(stringResource(body), style = MaterialTheme.typography.bodyMedium, color = LocalBrandPalette.current.text2)
    }
}

@Composable
private fun RequestsMenu(requests: RequestsState, actions: RequestsActions) {
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
        MenuItem(R.string.requests_clear, close, actions.clear)
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
    val corner = MaterialTheme.shapes.medium.topStart
    val none = CornerSize(0.dp)
    Surface(
        shape = RoundedCornerShape(
            topStart = if (first) corner else none,
            topEnd = if (first) corner else none,
            bottomStart = if (last) corner else none,
            bottomEnd = if (last) corner else none,
        ),
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

package io.anixops.net.ui

import android.text.format.DateFormat as AndroidDateFormat
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.anixops.net.R
import io.anixops.net.core.LogEntry
import io.anixops.net.core.TrafficBar
import io.anixops.net.core.TrafficShare
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette
import java.text.DateFormat
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale

/** What the two activity pages can ask for. */
data class ActivityActions(
    val back: () -> Unit = {},
    val setLevel: (String) -> Unit = {},
    val setModule: (String) -> Unit = {},
    val setQuery: (String) -> Unit = {},
    val refreshLogs: () -> Unit = {},
    val copyLogs: () -> Unit = {},
    val clearLogs: () -> Unit = {},
    val setPeriod: (String) -> Unit = {},
    val setView: (StatsView) -> Unit = {},
    val clearTraffic: () -> Unit = {},
)

/** How many lines the log page shows at first, and adds each time. */
private const val LogPage = 100

private val LogLevels = listOf("all" to R.string.logs_all, "warn" to R.string.logs_warnings, "error" to R.string.logs_errors)

/** 设置 › 日志: the app's own log, filtered by level, module and text, newest first. */
@Composable
fun LogsScreen(state: LogsState, actions: ActivityActions, modifier: Modifier = Modifier) {
    var confirmClear by remember { mutableStateOf(false) }
    var limit by rememberSaveable { mutableIntStateOf(LogPage) }
    val logs = state.logs
    Page(stringResource(R.string.logs), modifier, onBack = actions.back) {
        Column {
            Row(verticalAlignment = Alignment.CenterVertically) {
                SingleChoiceSegmentedButtonRow(modifier = Modifier.weight(1f)) {
                    LogLevels.forEachIndexed { index, (level, label) ->
                        SegmentedButton(
                            selected = state.level == level,
                            onClick = {
                                limit = LogPage
                                actions.setLevel(level)
                            },
                            shape = SegmentedButtonDefaults.itemShape(index, LogLevels.size),
                        ) { Text(stringResource(label)) }
                    }
                }
                LogsMenu(
                    canCopy = logs?.shown?.isNotEmpty() == true,
                    onRefresh = actions.refreshLogs,
                    onCopy = actions.copyLogs,
                    onClear = { confirmClear = true },
                )
            }
            if ((logs?.modules?.size ?: 0) > 1) {
                ModulePicker(logs?.modules.orEmpty(), state.module) {
                    limit = LogPage
                    actions.setModule(it)
                }
            }
        }
        OutlinedTextField(
            value = state.query,
            onValueChange = {
                limit = LogPage
                actions.setQuery(it)
            },
            singleLine = true,
            leadingIcon = { Icon(Icons.Filled.Search, contentDescription = null) },
            placeholder = { Text(stringResource(R.string.logs_filter)) },
            modifier = Modifier.fillMaxWidth(),
        )
        Column {
            Section(title = null) {
                val shown = logs?.shown.orEmpty()
                if (shown.isEmpty()) {
                    Text(
                        stringResource(if (logs?.modules.isNullOrEmpty()) R.string.logs_none else R.string.logs_no_match),
                        style = MaterialTheme.typography.bodyMedium,
                        color = LocalBrandPalette.current.text2,
                        modifier = Modifier.padding(BrandSpacing.md),
                    )
                }
                shown.take(limit).forEachIndexed { index, entry ->
                    if (index > 0) RowDivider()
                    LogLine(entry)
                }
                if (shown.size > limit) {
                    RowDivider()
                    val more = minOf(LogPage, shown.size - limit)
                    TextButton(onClick = { limit += LogPage }, modifier = Modifier.padding(horizontal = BrandSpacing.xs)) {
                        Text(pluralStringResource(R.plurals.logs_show_more, more, more))
                    }
                }
            }
            Note(stringResource(R.string.logs_note))
        }
    }
    if (confirmClear) {
        AlertDialog(
            onDismissRequest = { confirmClear = false },
            title = { Text(stringResource(R.string.logs_confirm_clear)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmClear = false
                    actions.clearLogs()
                }) { Text(stringResource(R.string.action_clear), color = LocalBrandPalette.current.danger) }
            },
            dismissButton = { TextButton(onClick = { confirmClear = false }) { Text(stringResource(R.string.action_cancel)) } },
        )
    }
}

@Composable
private fun LogsMenu(canCopy: Boolean, onRefresh: () -> Unit, onCopy: () -> Unit, onClear: () -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }) {
            Icon(Icons.Filled.MoreVert, contentDescription = stringResource(R.string.logs_more))
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            DropdownMenuItem(text = { Text(stringResource(R.string.logs_refresh)) }, onClick = {
                open = false
                onRefresh()
            })
            DropdownMenuItem(text = { Text(stringResource(R.string.logs_copy)) }, enabled = canCopy, onClick = {
                open = false
                onCopy()
            })
            DropdownMenuItem(
                text = { Text(stringResource(R.string.logs_clear), color = LocalBrandPalette.current.danger) },
                onClick = {
                    open = false
                    onClear()
                },
            )
        }
    }
}

@Composable
private fun ModulePicker(modules: List<String>, selected: String, onChoose: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        TextButton(onClick = { open = true }) {
            Text(if (selected.isEmpty()) stringResource(R.string.logs_all_modules) else moduleLabel(selected))
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            (listOf("") + modules).forEach { module ->
                DropdownMenuItem(
                    text = { Text(if (module.isEmpty()) stringResource(R.string.logs_all_modules) else moduleLabel(module)) },
                    trailingIcon = { if (module == selected) Icon(Icons.Filled.Check, contentDescription = null) },
                    onClick = {
                        open = false
                        onChoose(module)
                    },
                )
            }
        }
    }
}

/** The modules the app names; others are shown as the client writes them (`LogsCard.tsx`). */
@Composable
private fun moduleLabel(module: String): String = when (module) {
    "app" -> stringResource(R.string.logs_module_app)
    "gui" -> stringResource(R.string.logs_module_window)
    "mitm" -> stringResource(R.string.logs_module_mitm)
    else -> module
}

@Composable
private fun LogLine(entry: LogEntry) {
    val palette = LocalBrandPalette.current
    val locale = LocalConfiguration.current.locales[0]
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = RowMinHeight)
            .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.xs),
        verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs),
    ) {
        Row(horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
            Text(
                if (entry.timeMs > 0) logTime(entry.timeMs, locale) else "-",
                style = MaterialTheme.typography.labelMedium,
                color = palette.text2,
            )
            Text(moduleLabel(entry.module), style = MaterialTheme.typography.labelMedium, color = palette.text2)
        }
        Text(
            entry.message,
            style = MaterialTheme.typography.bodySmall.copy(fontFamily = FontFamily.Monospace),
            color = when (entry.level) {
                "error" -> palette.danger
                "warn" -> palette.warning
                else -> palette.text
            },
        )
    }
}

private fun logTime(ms: Long, locale: Locale): String =
    DateFormat.getDateTimeInstance(DateFormat.SHORT, DateFormat.MEDIUM, locale).format(Date(ms))

/** 设置 › 流量统计: today or the last seven days, over time, by host or by policy. */
@Composable
fun TrafficScreen(state: TrafficState, actions: ActivityActions, modifier: Modifier = Modifier) {
    var confirmClear by remember { mutableStateOf(false) }
    val page = state.page
    val view = page?.view
    val palette = LocalBrandPalette.current
    Page(stringResource(R.string.traffic_stats), modifier, onBack = actions.back) {
        Choice(
            listOf("today" to stringResource(R.string.traffic_today), "week" to stringResource(R.string.traffic_week)),
            state.period,
            actions.setPeriod,
        )
        Choice(
            listOf(
                StatsView.Time to stringResource(if (state.period == "today") R.string.traffic_by_hour else R.string.traffic_by_day),
                StatsView.Hosts to stringResource(R.string.traffic_by_host),
                StatsView.Policies to stringResource(R.string.traffic_by_policy),
            ),
            state.view,
            actions.setView,
        )
        Column {
            Section(
                title = null,
                action = null,
            ) {
                Row(
                    modifier = Modifier.padding(BrandSpacing.md),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
                        Text(formatBytes((view?.upload ?: 0) + (view?.download ?: 0)), style = MaterialTheme.typography.titleLarge)
                        Text(
                            stringResource(
                                R.string.traffic_up_down,
                                formatBytes(view?.upload ?: 0),
                                formatBytes(view?.download ?: 0),
                            ),
                            style = MaterialTheme.typography.bodySmall,
                            color = palette.text2,
                        )
                    }
                    TextButton(onClick = { confirmClear = true }, enabled = page?.recorded == true) {
                        Text(stringResource(R.string.traffic_clear), color = if (page?.recorded == true) palette.danger else palette.text2)
                    }
                }
                RowDivider()
                when {
                    view == null || view.upload + view.download == 0L -> Text(
                        stringResource(if (page?.recorded == true) R.string.traffic_none else R.string.traffic_after_connect),
                        style = MaterialTheme.typography.bodyMedium,
                        color = palette.text2,
                        modifier = Modifier.padding(BrandSpacing.md),
                    )
                    state.view == StatsView.Time -> Bars(view.bars)
                    else -> Shares(if (state.view == StatsView.Hosts) view.hosts else view.policies)
                }
            }
            Note(stringResource(R.string.traffic_note))
        }
    }
    if (confirmClear) {
        AlertDialog(
            onDismissRequest = { confirmClear = false },
            title = { Text(stringResource(R.string.traffic_confirm_clear)) },
            confirmButton = {
                TextButton(onClick = {
                    confirmClear = false
                    actions.clearTraffic()
                }) { Text(stringResource(R.string.action_clear), color = palette.danger) }
            },
            dismissButton = { TextButton(onClick = { confirmClear = false }) { Text(stringResource(R.string.action_cancel)) } },
        )
    }
}

@Composable
private fun <T> Choice(options: List<Pair<T, String>>, selected: T, onChoose: (T) -> Unit) {
    SingleChoiceSegmentedButtonRow(modifier = Modifier.fillMaxWidth()) {
        options.forEachIndexed { index, (value, label) ->
            SegmentedButton(
                selected = value == selected,
                onClick = { onChoose(value) },
                shape = SegmentedButtonDefaults.itemShape(index, options.size),
            ) { Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis) }
        }
    }
}

private val ChartHeight = 120.dp
private val BarRadius = 2.dp

/** The bars of the period, oldest first, scaled to the largest, with the first, middle and last bar's time under them. */
@Composable
private fun Bars(bars: List<TrafficBar>) {
    val bytes = bars.map { it.bytes }
    val color = LocalBrandPalette.current.primary
    val track = LocalBrandPalette.current.border
    val label = stringResource(R.string.traffic_stats)
    val peak = (bytes.maxOrNull() ?: 0L).coerceAtLeast(1L)
    Canvas(
        modifier = Modifier
            .fillMaxWidth()
            .padding(BrandSpacing.md)
            .height(ChartHeight)
            .semantics { contentDescription = label },
    ) {
        if (bytes.isEmpty()) return@Canvas
        val slot = size.width / bytes.size
        val gap = slot * GAP_SHARE
        val radius = CornerRadius(BarRadius.toPx())
        drawLine(track, Offset(0f, size.height), Offset(size.width, size.height))
        bytes.forEachIndexed { index, value ->
            val height = size.height * value / peak
            if (height > 0f) {
                drawRoundRect(
                    color,
                    topLeft = Offset(index * slot + gap / 2, size.height - height),
                    size = Size(slot - gap, height),
                    cornerRadius = radius,
                )
            }
        }
    }
    if (bars.isEmpty()) return
    val locale = LocalConfiguration.current.locales[0]
    // Hours for a day's 24 bars, days for a week's 7.
    val format = remember(locale, bars.size) {
        SimpleDateFormat(AndroidDateFormat.getBestDateTimePattern(locale, if (bars.size > DAYS_IN_WEEK) "Hm" else "Md"), locale)
    }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(start = BrandSpacing.md, end = BrandSpacing.md, bottom = BrandSpacing.sm),
        horizontalArrangement = Arrangement.SpaceBetween,
    ) {
        listOf(bars.first(), bars[bars.size / 2], bars.last()).forEach { bar ->
            Text(
                format.format(Date(bar.start * MILLIS_PER_SECOND)),
                style = MaterialTheme.typography.labelSmall,
                color = LocalBrandPalette.current.text2,
            )
        }
    }
}

private const val DAYS_IN_WEEK = 7
private const val MILLIS_PER_SECOND = 1_000L

private const val GAP_SHARE = 0.25f

@Composable
private fun Shares(rows: List<TrafficShare>) {
    rows.forEachIndexed { index, row ->
        if (index > 0) RowDivider()
        InfoRow(if (row.name == "*") stringResource(R.string.traffic_other_hosts) else row.name, formatBytes(row.bytes))
    }
}

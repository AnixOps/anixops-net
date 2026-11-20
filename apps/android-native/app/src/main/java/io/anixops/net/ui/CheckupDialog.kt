package io.anixops.net.ui

import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.unit.dp
import io.anixops.net.R
import io.anixops.net.core.CheckupRow
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette
import java.util.Locale
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.doubleOrNull
import kotlinx.serialization.json.longOrNull

/**
 * 节点体检 of one node: a switch for the download, the button that starts it, and what it found. The rows and which
 * are marked come from the core; this words them and writes the values.
 */
@Composable
fun CheckupDialog(checkup: CheckupState, onRun: (Boolean) -> Unit, onDismiss: () -> Unit) {
    val palette = LocalBrandPalette.current
    var throughput by rememberSaveable { mutableStateOf(false) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(stringResource(R.string.proxies_checkup)) },
        text = {
            Column(
                modifier = Modifier.verticalScroll(rememberScrollState()),
                verticalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
            ) {
                Text(checkup.label, style = MaterialTheme.typography.titleSmall)
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .heightIn(min = 48.dp)
                        .toggleable(
                            value = throughput,
                            enabled = !checkup.running,
                            role = Role.Switch,
                            onValueChange = { throughput = it },
                        ),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(stringResource(R.string.checkup_throughput), modifier = Modifier.weight(1f))
                    Switch(checked = throughput, onCheckedChange = null, enabled = !checkup.running)
                }
                Text(
                    stringResource(R.string.checkup_note) +
                        if (throughput) " " + stringResource(R.string.checkup_note_throughput) else "",
                    style = MaterialTheme.typography.bodySmall,
                    color = palette.text2,
                )
                OutlinedButton(onClick = { onRun(throughput) }, enabled = !checkup.running) {
                    Text(
                        stringResource(
                            when {
                                checkup.running -> R.string.checkup_running
                                checkup.result != null || checkup.problem != null -> R.string.checkup_again
                                else -> R.string.checkup_start
                            },
                        ),
                    )
                }
                checkup.problem?.let { Text(it, color = palette.danger, style = MaterialTheme.typography.bodyMedium) }
                checkup.result?.sections?.forEach { section ->
                    val heading = sectionTitle(section.id) ?: return@forEach
                    Text(heading, style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(top = BrandSpacing.xs))
                    section.rows.forEach { row ->
                        val label = rowLabel(row.id) ?: return@forEach
                        Row(modifier = Modifier.fillMaxWidth()) {
                            Text(stringResource(label), modifier = Modifier.weight(1f), style = MaterialTheme.typography.bodyMedium)
                            Text(
                                checkupValue(row),
                                style = MaterialTheme.typography.bodyMedium,
                                color = if (row.warn) palette.danger else palette.text2,
                            )
                        }
                    }
                    if (section.capped) {
                        Text(stringResource(R.string.checkup_capped), style = MaterialTheme.typography.bodySmall, color = palette.text2)
                    }
                }
            }
        },
        confirmButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_close)) } },
    )
}

@Composable
private fun sectionTitle(id: String): String? = when (id) {
    "latency" -> stringResource(R.string.checkup_section_latency)
    "download" -> stringResource(R.string.checkup_section_download)
    else -> null
}

@StringRes
private fun rowLabel(id: String): Int? = when (id) {
    "handshake" -> R.string.checkup_row_handshake
    "cold_handshake" -> R.string.checkup_row_cold_handshake
    "warm_handshake" -> R.string.checkup_row_warm_handshake
    "request" -> R.string.checkup_row_request
    "jitter" -> R.string.checkup_row_jitter
    "loss" -> R.string.checkup_row_loss
    "failure" -> R.string.checkup_row_failure
    "mbps" -> R.string.checkup_row_mbps
    "peak_mbps" -> R.string.checkup_row_peak_mbps
    "bytes" -> R.string.checkup_row_bytes
    "idle" -> R.string.checkup_row_idle
    "loaded" -> R.string.checkup_row_loaded
    "bloat" -> R.string.checkup_row_bloat
    "loaded_loss" -> R.string.checkup_row_loaded_loss
    else -> null
}

@Composable
private fun checkupValue(row: CheckupRow): String {
    val value = row.value as? JsonPrimitive
    if (row.kind == "failure") {
        val code = value?.takeIf { it.isString }?.content ?: return NONE
        return when (code) {
            "timeout" -> stringResource(R.string.checkup_failure_timeout)
            "connect_failed" -> stringResource(R.string.checkup_failure_connect_failed)
            "tls_failed" -> stringResource(R.string.checkup_failure_tls_failed)
            "not_http" -> stringResource(R.string.checkup_failure_not_http)
            "http_status" -> stringResource(R.string.checkup_failure_http_status)
            else -> code
        }
    }
    return formatCheckupValue(row.kind, value?.takeIf { !it.isString }?.doubleOrNull, value?.longOrNull)
}

private const val NONE = "—"

/** A measure with its unit, as the desktop windows write it; a dash when no sample succeeded. */
fun formatCheckupValue(kind: String, number: Double?, whole: Long? = null): String {
    if (number == null) return NONE
    return when (kind) {
        "ms" -> String.format(Locale.ROOT, if (number < 10) "%.1f ms" else "%.0f ms", number)
        "pct" -> String.format(Locale.ROOT, "%.0f%%", number)
        "mbps" -> String.format(Locale.ROOT, if (number < 10) "%.2f Mbps" else "%.1f Mbps", number)
        "bytes" -> formatBytes(whole ?: number.toLong().coerceAtLeast(0))
        else -> number.toString()
    }
}

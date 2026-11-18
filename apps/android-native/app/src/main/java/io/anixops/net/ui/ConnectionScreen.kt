package io.anixops.net.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
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
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.style.TextOverflow
import io.anixops.net.R
import io.anixops.net.core.CaptureChunk
import io.anixops.net.core.CaptureConnection
import io.anixops.net.core.CaptureDirection
import io.anixops.net.core.CaptureExchange
import io.anixops.net.core.DecryptHost
import io.anixops.net.core.MappingText
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette
import java.text.DateFormat
import java.util.Date
import java.util.Locale

/**
 * One connection opened from the connections list (调试 › 请求): how much went each way, its decrypted requests, its
 * reads and writes as text or hex, and its export. TLS data says how to get it decrypted, with the way to do it.
 */
@Composable
fun ConnectionScreen(view: ConnectionView, requests: RequestsState, actions: RequestsActions, modifier: Modifier = Modifier) {
    Page(view.name, modifier, actions.closeConnection) {
        val capture = view.capture
        when {
            capture != null -> ConnectionContent(capture, view.decrypt, requests, actions)
            view.gone -> Note(stringResource(R.string.connection_gone))
            else -> Note(stringResource(R.string.request_loading))
        }
    }
}

@Composable
private fun ConnectionContent(capture: CaptureConnection, decrypt: DecryptHost?, requests: RequestsState, actions: RequestsActions) {
    Column {
        Section(title = null) {
            InfoRow(stringResource(R.string.connection_upload), directionText(capture.upload))
            InfoRow(stringResource(R.string.connection_download), directionText(capture.download), divider = true)
            InfoRow(stringResource(R.string.connection_target), "${capture.network.uppercase(Locale.ROOT)} ${capture.destination}", divider = true)
            if (capture.decrypted) {
                InfoRow(stringResource(R.string.connection_content), stringResource(R.string.connection_decrypted), divider = true)
            }
        }
        Note(stringResource(R.string.connection_kept_note))
    }
    if (capture.http.isNotEmpty()) {
        Section(stringResource(R.string.connection_requests)) {
            val listed = requests.rows.filter { it.connectionId == capture.id }.map { it.index }.toSet()
            capture.http.forEachIndexed { index, exchange ->
                if (index > 0) RowDivider()
                ExchangeLine(exchange, onClick = if (index in listed) ({ actions.openExchange(index) }) else null)
            }
        }
    }
    if (capture.chunks.isNotEmpty()) {
        val tls = CaptureBytes.looksLikeTls(capture)
        Column {
            val canDecrypt = tls && decrypt != null && decrypt.decryptable && !decrypt.covered && !decrypt.excluded
            val decryptAction: (@Composable () -> Unit)? = if (canDecrypt) {
                { TextButton(onClick = actions.decryptConnectionHost) { Text(stringResource(R.string.connection_decrypt_host)) } }
            } else {
                null
            }
            Section(
                stringResource(if (capture.network == "udp") R.string.connection_datagrams else R.string.connection_data),
                action = decryptAction,
            ) {
                capture.chunks.forEachIndexed { index, chunk ->
                    if (index > 0) RowDivider()
                    ChunkLine(chunk)
                }
            }
            if (tls) Note(stringResource(tlsNote(decrypt)))
        }
    }
    if (capture.http.isEmpty() && capture.chunks.isEmpty()) Note(stringResource(R.string.connection_nothing))
    Column {
        Section(stringResource(R.string.connection_export)) {
            Row(
                modifier = Modifier.padding(BrandSpacing.md),
                horizontalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
            ) {
                OutlinedButton(onClick = { actions.exportConnection("pcapng") }) { Text(stringResource(R.string.connection_export_pcapng)) }
                if (capture.http.isNotEmpty()) {
                    OutlinedButton(onClick = { actions.exportConnection("har") }) { Text(stringResource(R.string.connection_export_har)) }
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
        }
        Note(stringResource(R.string.connection_export_note))
    }
}

/** What to do about TLS data: nothing for an address, wait for a new connection once covered, else add the domain. */
private fun tlsNote(decrypt: DecryptHost?): Int = when {
    decrypt == null || !decrypt.decryptable -> R.string.connection_tls_address
    decrypt.excluded -> R.string.connection_tls_excluded
    decrypt.covered -> R.string.connection_tls_covered
    else -> R.string.connection_tls_add
}

@Composable
private fun directionText(direction: CaptureDirection): String {
    val kept = formatBytes(direction.captured)
    return if (direction.truncated) stringResource(R.string.connection_kept, kept, formatBytes(direction.seen)) else kept
}

@Composable
private fun ExchangeLine(exchange: CaptureExchange, onClick: (() -> Unit)?) {
    Column(
        modifier = Modifier
            .fillMaxWidth()
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .heightIn(min = RowMinHeight)
            .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.sm),
        verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs),
    ) {
        Text(
            "${exchange.method} ${exchange.url}",
            style = MaterialTheme.typography.bodyMedium,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
        )
        val took = exchange.endedAtMs?.let { formatDuration(maxOf(0, it - exchange.startedAtMs)) }
        Text(
            listOfNotNull(
                exchange.status?.toString() ?: stringResource(R.string.connection_no_response),
                took,
                exchange.response.body.size.takeIf { it > 0 }?.let(::formatBytes),
                exchange.response.header("content-type"),
            ).joinToString(" · "),
            style = MaterialTheme.typography.bodySmall,
            color = LocalBrandPalette.current.text2,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

/** One read or write; a tap shows its bytes, as text when they are text, and hex on request. */
@Composable
private fun ChunkLine(chunk: CaptureChunk) {
    var open by rememberSaveable(chunk.atMs, chunk.dir) { mutableStateOf(false) }
    val bytes = remember(chunk.dataBase64) { MappingText.bytes(chunk.dataBase64) }
    val locale = LocalConfiguration.current.locales[0]
    Column {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .clickable { open = !open }
                .heightIn(min = RowMinHeight)
                .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.sm),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            val upload = chunk.dir == "upload"
            Text(
                stringResource(if (upload) R.string.connection_sent else R.string.connection_received),
                style = MaterialTheme.typography.bodyLarge,
                color = if (upload) LocalBrandPalette.current.warning else LocalBrandPalette.current.text,
                modifier = Modifier.weight(1f),
            )
            Text(
                formatBytes(bytes.size.toLong()) + " · " + DateFormat.getTimeInstance(DateFormat.MEDIUM, locale).format(Date(chunk.atMs)),
                style = MaterialTheme.typography.bodyMedium,
                color = LocalBrandPalette.current.text2,
            )
        }
        if (open) BytesBox(bytes)
    }
}

@Composable
private fun BytesBox(bytes: ByteArray) {
    val text = remember(bytes) { CaptureBytes.text(bytes) }
    var hex by rememberSaveable { mutableStateOf(text == null) }
    val shown = remember(bytes, hex) { if (hex || text == null) CaptureBytes.hexDump(bytes) else text }
    Column(Modifier.padding(start = BrandSpacing.md, end = BrandSpacing.md, bottom = BrandSpacing.sm)) {
        if (text != null) {
            TextButton(onClick = { hex = !hex }) {
                Text(stringResource(if (hex) R.string.bytes_text else R.string.bytes_hex))
            }
        }
        SelectionContainer {
            Text(
                shown.ifEmpty { " " },
                style = MaterialTheme.typography.bodySmall,
                fontFamily = FontFamily.Monospace,
                softWrap = !hex,
                modifier = if (hex) Modifier.horizontalScroll(rememberScrollState()) else Modifier,
            )
        }
    }
}

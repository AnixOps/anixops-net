package io.anixops.net.ui

import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.font.FontFamily
import io.anixops.net.R
import io.anixops.net.core.CaptureMessage
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette
import java.text.DateFormat
import java.util.Date

/** One request opened from the list: what it was, then its headers and bodies, the request's before the response's. */
@Composable
fun RequestDetailScreen(opened: OpenedRequest, onBack: () -> Unit, modifier: Modifier = Modifier) {
    val exchange = opened.exchange
    Page(stringResource(R.string.request_detail), modifier, onBack) {
        SummarySection(opened)
        when {
            exchange != null -> {
                MessageSections(R.string.request_headers, R.string.request_body, exchange.request, opened.requestBody)
                MessageSections(R.string.response_headers, R.string.response_body, exchange.response, opened.responseBody)
            }
            opened.gone -> Note(stringResource(R.string.request_gone))
            else -> Note(stringResource(R.string.request_loading))
        }
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
private fun MessageSections(@StringRes headersTitle: Int, @StringRes bodyTitle: Int, message: CaptureMessage, body: BodyText?) {
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
    if (body != null) BodySection(stringResource(bodyTitle), body)
}

@Composable
private fun BodySection(title: String, body: BodyText) {
    val palette = LocalBrandPalette.current
    Column {
        Section("$title · ${formatBytes(body.size)}") {
            val text = body.text
            when {
                body.size == 0L -> Text(
                    stringResource(R.string.request_no_body),
                    style = MaterialTheme.typography.bodyMedium,
                    color = palette.text2,
                    modifier = Modifier.padding(BrandSpacing.md),
                )
                text == null -> Text(
                    stringResource(R.string.request_binary_body),
                    style = MaterialTheme.typography.bodyMedium,
                    color = palette.text2,
                    modifier = Modifier.padding(BrandSpacing.md),
                )
                else -> SelectionContainer {
                    Text(
                        text,
                        style = MaterialTheme.typography.bodySmall,
                        fontFamily = FontFamily.Monospace,
                        modifier = Modifier.padding(BrandSpacing.md),
                    )
                }
            }
        }
        if (body.truncated) Note(stringResource(R.string.request_body_truncated))
        if (body.undecoded) Note(stringResource(R.string.request_body_undecoded))
    }
}

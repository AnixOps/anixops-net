package io.anixops.net.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import io.anixops.net.R
import io.anixops.net.core.CaptureSession
import io.anixops.net.core.CaptureStatus
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette
import java.text.DateFormat
import java.util.Date
import java.util.Locale

/** Longest session name the engine keeps. */
private const val MaxNameChars = 120

/** A session's title: its name, or when it was saved. */
fun sessionTitle(session: CaptureSession, locale: Locale): String =
    session.name?.trim()?.takeIf { it.isNotEmpty() }
        ?: DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT, locale).format(Date(session.savedAtMs))

/**
 * Saved capture sessions (调试 › 请求 › 抓包会话): seal what capture holds into an encrypted file on this device, load
 * one back to look at its requests and export them, or delete one. The engine seals them; the key stays in the
 * Android Keystore.
 */
@Composable
fun SessionsScreen(capture: CaptureStatus?, page: SessionsPage, actions: RequestsActions, modifier: Modifier = Modifier) {
    val captured = capture?.connections ?: 0
    var loading by remember { mutableStateOf<CaptureSession?>(null) }
    var deleting by remember { mutableStateOf<CaptureSession?>(null) }
    val locale = LocalConfiguration.current.locales[0]
    Page(stringResource(R.string.sessions), modifier, actions.closeSessions) {
        Section(stringResource(R.string.sessions_save)) {
            Column(
                modifier = Modifier.padding(BrandSpacing.md),
                verticalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
            ) {
                Text(
                    if (captured > 0) {
                        pluralStringResource(R.plurals.sessions_connections, captured, captured)
                    } else {
                        stringResource(R.string.sessions_nothing)
                    },
                    style = MaterialTheme.typography.bodyMedium,
                    color = LocalBrandPalette.current.text2,
                )
                OutlinedTextField(
                    value = page.name,
                    onValueChange = { actions.setSessionName(it.take(MaxNameChars)) },
                    label = { Text(stringResource(R.string.sessions_name)) },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                    modifier = Modifier.fillMaxWidth(),
                )
                Button(onClick = actions.saveSession, enabled = captured > 0 && !page.working) {
                    Text(stringResource(R.string.action_save))
                }
            }
        }
        Column {
            Section(stringResource(R.string.sessions_saved)) {
                when {
                    !page.loaded -> SessionHint(stringResource(R.string.sessions_loading))
                    page.failure != null -> SessionHint(page.failure, LocalBrandPalette.current.danger)
                    page.sessions.isEmpty() -> SessionHint(stringResource(R.string.sessions_empty))
                    else -> page.sessions.forEachIndexed { index, session ->
                        if (index > 0) RowDivider()
                        SessionRow(
                            session,
                            locale,
                            enabled = !page.working,
                            onLoad = { if (captured > 0) loading = session else actions.loadSession(session) },
                            onDelete = { deleting = session },
                        )
                    }
                }
            }
            Note(protectionText(page.protection, page.retentionDays))
        }
    }
    loading?.let { session ->
        AlertDialog(
            onDismissRequest = { loading = null },
            title = { Text(stringResource(R.string.sessions_replace_title, sessionTitle(session, locale))) },
            text = { Text(stringResource(R.string.sessions_replace_body)) },
            confirmButton = {
                TextButton(
                    onClick = {
                        loading = null
                        actions.loadSession(session)
                    },
                ) { Text(stringResource(R.string.sessions_replace)) }
            },
            dismissButton = { TextButton(onClick = { loading = null }) { Text(stringResource(R.string.action_cancel)) } },
        )
    }
    deleting?.let { session ->
        ConfirmDialog(
            stringResource(R.string.sessions_delete_confirm, sessionTitle(session, locale)),
            onConfirm = {
                deleting = null
                actions.deleteSession(session)
            },
            onDismiss = { deleting = null },
        )
    }
}

@Composable
private fun SessionHint(text: String, color: Color = LocalBrandPalette.current.text2) {
    Text(
        text,
        style = MaterialTheme.typography.bodyMedium,
        color = color,
        modifier = Modifier.padding(BrandSpacing.md),
    )
}

@Composable
private fun SessionRow(session: CaptureSession, locale: Locale, enabled: Boolean, onLoad: () -> Unit, onDelete: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = RowMinHeight)
            .padding(start = BrandSpacing.md, end = BrandSpacing.xs, top = BrandSpacing.sm, bottom = BrandSpacing.sm),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
            Text(sessionTitle(session, locale), style = MaterialTheme.typography.bodyLarge)
            Text(sessionHint(session, locale), style = MaterialTheme.typography.bodySmall, color = LocalBrandPalette.current.text2)
        }
        if (!session.unreadable) {
            TextButton(onClick = onLoad, enabled = enabled) { Text(stringResource(R.string.sessions_load)) }
        }
        IconButton(onClick = onDelete, enabled = enabled) {
            Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.sessions_delete), tint = LocalBrandPalette.current.text2)
        }
    }
}

/** What is in a session and when it goes, or why it cannot be opened. */
@Composable
private fun sessionHint(session: CaptureSession, locale: Locale): String {
    if (session.unreadable) return stringResource(R.string.sessions_unreadable)
    return listOf(
        pluralStringResource(R.plurals.requests_count, session.requests, session.requests),
        pluralStringResource(R.plurals.sessions_connections, session.connections, session.connections),
        formatBytes(session.size),
        stringResource(
            R.string.sessions_expires,
            DateFormat.getDateInstance(DateFormat.MEDIUM, locale).format(Date(session.expiresAtMs)),
        ),
    ).joinToString(" · ")
}

/** Where the sessions stay, for how long, and what keeps their key. */
@Composable
private fun protectionText(protection: String, days: Int): String {
    val kept = pluralStringResource(R.plurals.sessions_kept, days, days)
    return if (protection == "keystore") kept + stringResource(R.string.sessions_keystore) else kept
}

package io.anixops.net.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.unit.dp
import io.anixops.net.R
import io.anixops.net.core.MappingText
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette

/** Width of the method field, room for "OPTIONS". */
private val MethodWidth = 112.dp

/** Height the header and body boxes start at, a few lines each. */
private val BoxMinHeight = 120.dp

/**
 * Sends an edited copy of the opened request again through the engine, so it leaves through the current exit and
 * shows up in capture. Redirects are not followed and no cookies are kept.
 */
@Composable
fun ReplayScreen(draft: ReplayDraft, actions: RequestsActions, modifier: Modifier = Modifier) {
    val mono = MaterialTheme.typography.bodySmall.merge(TextStyle(fontFamily = FontFamily.Monospace))
    Page(stringResource(R.string.replay), modifier, actions.closeReplay) {
        Column {
            Section(title = null) {
                Column(
                    modifier = Modifier.padding(BrandSpacing.md),
                    verticalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
                ) {
                    Row(horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs), verticalAlignment = Alignment.Top) {
                        OutlinedTextField(
                            value = draft.method,
                            onValueChange = { text -> actions.editReplay { it.copy(method = text.uppercase()) } },
                            label = { Text(stringResource(R.string.replay_method)) },
                            singleLine = true,
                            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Characters),
                            modifier = Modifier.width(MethodWidth),
                        )
                        OutlinedTextField(
                            value = draft.url,
                            onValueChange = { text -> actions.editReplay { it.copy(url = text) } },
                            label = { Text(stringResource(R.string.replay_url)) },
                            singleLine = true,
                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                            modifier = Modifier.weight(1f),
                        )
                    }
                    OutlinedTextField(
                        value = draft.headers,
                        onValueChange = { text -> actions.editReplay { it.copy(headers = text) } },
                        label = { Text(stringResource(R.string.replay_headers)) },
                        textStyle = mono,
                        modifier = Modifier
                            .fillMaxWidth()
                            .heightIn(min = BoxMinHeight),
                    )
                    val body = draft.body
                    if (body != null) {
                        OutlinedTextField(
                            value = body,
                            onValueChange = { text -> actions.editReplay { it.copy(body = text) } },
                            label = { Text(stringResource(if (draft.cut) R.string.replay_body_cut else R.string.replay_body)) },
                            textStyle = mono,
                            modifier = Modifier
                                .fillMaxWidth()
                                .heightIn(min = BoxMinHeight),
                        )
                    } else {
                        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(BrandSpacing.sm)) {
                            Text(
                                stringResource(
                                    R.string.replay_binary_body,
                                    formatBytes(MappingText.bytes(draft.binaryBase64).size.toLong()),
                                ),
                                style = MaterialTheme.typography.bodyMedium,
                                color = LocalBrandPalette.current.text2,
                                modifier = Modifier.weight(1f),
                            )
                            OutlinedButton(onClick = { actions.editReplay { it.copy(body = "", binaryBase64 = "") } }) {
                                Text(stringResource(R.string.replay_clear_body))
                            }
                        }
                    }
                    Button(onClick = actions.sendReplay, enabled = !draft.sending && draft.url.isNotBlank()) {
                        Text(stringResource(if (draft.sending) R.string.replay_sending else R.string.replay_send))
                    }
                }
            }
            Note(stringResource(R.string.replay_note))
        }
        draft.answer?.let { answer ->
            Section(stringResource(R.string.replay_answer, answer.status)) {
                InfoRow(stringResource(R.string.request_duration), formatDuration(answer.elapsedMs))
            }
            MessageSections(R.string.response_headers, R.string.response_body, answer.message, draft.answerBody)
        }
    }
}

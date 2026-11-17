package io.anixops.net.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.anixops.net.R
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette

/** Test tag of the connection switch, on whichever page holds it (docs/ui-ux.md, "连接开关在每一页都在"). */
const val CONNECT_SWITCH_TAG = "connect-switch"

/** Test tag of the place a failure is shown on the current page. */
const val NOTICE_TAG = "app-notice"

/**
 * What went wrong last: a refused command or the VPN consent ([UiState.notice]) before the connection's own error.
 * Null when nothing did.
 */
fun UiState.problem(): String? = notice ?: overview?.error

/**
 * Debug mode's connection switch. Debug mode has no 首页, where the proxy client keeps it, so it sits in a bar under
 * every page instead, with the state, where the traffic leaves and the last failure; the same place on 请求, 改写,
 * 出口, 设置 and the pages opened from them, like the iPhone's bar and the desktop sidebar's switch (scope S1).
 */
@Composable
fun DebugConnectionBar(state: UiState, actions: HomeActions, modifier: Modifier = Modifier) {
    val palette = LocalBrandPalette.current
    val overview = state.overview
    val connected = overview?.state == "connected"
    val status = if (state.busy) stringResource(R.string.status_working) else statusLabel(overview?.state ?: "disconnected")
    val problem = state.problem()
    val dot = when {
        connected -> palette.success
        overview?.state == "failed" || overview?.state == "core_error" -> palette.danger
        else -> palette.text3
    }
    val switchLabel = stringResource(R.string.home_connection)
    Surface(color = palette.surface, modifier = modifier.fillMaxWidth()) {
        Column {
            HorizontalDivider(color = palette.border)
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = RowMinHeight)
                    .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.xs),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
            ) {
                Box(
                    Modifier
                        .size(8.dp)
                        .clip(CircleShape)
                        .background(dot),
                )
                Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
                    Text(status, style = MaterialTheme.typography.titleSmall, color = palette.text)
                    if (problem != null) {
                        Text(
                            problem,
                            style = MaterialTheme.typography.bodySmall,
                            color = palette.danger,
                            maxLines = 3,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.testTag(NOTICE_TAG),
                        )
                    } else {
                        Text(
                            when {
                                overview == null -> ""
                                overview.egress == "direct" -> stringResource(R.string.home_egress_direct)
                                else -> overview.node?.label ?: stringResource(R.string.home_no_node)
                            },
                            style = MaterialTheme.typography.bodySmall,
                            color = palette.text2,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
                if (state.notice != null) {
                    IconButton(onClick = actions.dismissNotice) {
                        Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.notice_dismiss))
                    }
                }
                Switch(
                    checked = connected,
                    enabled = !state.busy && overview != null && (connected || overview.canConnect),
                    onCheckedChange = { on -> if (on) actions.connect() else actions.disconnect() },
                    modifier = Modifier
                        .testTag(CONNECT_SWITCH_TAG)
                        .semantics {
                            contentDescription = switchLabel
                            stateDescription = status
                        },
                )
            }
        }
    }
}

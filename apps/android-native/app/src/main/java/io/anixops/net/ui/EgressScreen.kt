package io.anixops.net.ui

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import io.anixops.net.R

/** What 出口 needs beyond the proxies page: the exit choice and the way into 规则. */
data class EgressActions(
    val setExit: (DebugExit) -> Unit = {},
    val openRules: () -> Unit = {},
)

/**
 * 调试 › 出口 (docs/debug-mode.md G2): where captured traffic leaves, then the subscriptions and nodes of proxy mode,
 * and the rules and DNS one level down, as on the desktop client.
 */
@Composable
fun EgressScreen(
    exit: DebugExit,
    nodeCount: Int,
    proxiesState: ProxiesState,
    proxies: ProxiesActions,
    actions: EgressActions,
    modifier: Modifier = Modifier,
) {
    ProxiesScreen(proxiesState, proxies, modifier, title = stringResource(R.string.nav_egress)) {
        ExitSection(exit, nodeCount, actions.setExit)
        Section(title = null) {
            InfoRow(stringResource(R.string.egress_rules), stringResource(R.string.action_view), onClick = actions.openRules)
        }
    }
}

@Composable
private fun ExitSection(exit: DebugExit, nodeCount: Int, onChange: (DebugExit) -> Unit) {
    Column {
        Section(stringResource(R.string.nav_egress)) {
            Column(Modifier.selectableGroup()) {
                ChoiceRow(
                    stringResource(R.string.egress_direct),
                    stringResource(R.string.egress_direct_hint),
                    selected = exit == DebugExit.Direct,
                    enabled = true,
                    onClick = { onChange(DebugExit.Direct) },
                )
                RowDivider()
                ChoiceRow(
                    stringResource(R.string.egress_node),
                    stringResource(R.string.egress_node_hint),
                    selected = exit == DebugExit.Node,
                    enabled = true,
                    onClick = { onChange(DebugExit.Node) },
                )
            }
        }
        Note(
            stringResource(
                when {
                    exit == DebugExit.Direct -> R.string.egress_direct_note
                    nodeCount == 0 -> R.string.egress_no_nodes_note
                    else -> R.string.egress_node_note
                },
            ),
        )
    }
}

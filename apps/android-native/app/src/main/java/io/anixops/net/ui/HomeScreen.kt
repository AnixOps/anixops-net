package io.anixops.net.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import io.anixops.net.R
import io.anixops.net.core.ConnectionState
import io.anixops.net.core.RuntimeSnapshot
import io.anixops.net.ui.theme.Brand
import io.anixops.net.ui.theme.BrandPalette
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette
import java.net.URI

private val StatusDot = 10.dp

/** 首页: the connection switch on the brand hero card, then what the connection uses. */
@Composable
fun HomeScreen(
    state: UiState,
    onConnect: () -> Unit,
    onDisconnect: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val snapshot = state.snapshot
    Page(stringResource(R.string.nav_home), modifier) {
        ConnectionCard(snapshot, state.busy, onConnect, onDisconnect)
        val error = state.notice ?: snapshot?.lastError ?: snapshot?.configurationError
        if (error != null) ErrorCard(error)
        Section(title = null) {
            InfoRow(
                stringResource(R.string.home_node),
                snapshot?.selectedNode ?: stringResource(R.string.home_no_node),
            )
            InfoRow(
                stringResource(R.string.home_subscription),
                snapshot?.subscription?.let(::subscriptionName) ?: stringResource(R.string.home_no_subscription),
                divider = true,
            )
            InfoRow(
                stringResource(R.string.home_routing),
                routingLabel(snapshot?.routingMode),
                divider = true,
            )
        }
    }
}

/** The hero card (brand moment, E1): always the dark brand gradient, so its colours are the dark palette's. */
@Composable
private fun ConnectionCard(
    snapshot: RuntimeSnapshot?,
    busy: Boolean,
    onConnect: () -> Unit,
    onDisconnect: () -> Unit,
) {
    val palette = LocalBrandPalette.current
    val heroText = BrandPalette.Dark
    val state = snapshot?.state ?: ConnectionState.NotConnected
    val connected = state == ConnectionState.Connected
    val status = if (busy) stringResource(R.string.status_working) else statusLabel(state)
    val dot = when (state) {
        ConnectionState.Connected -> heroText.success
        ConnectionState.Failed, ConnectionState.CoreError -> heroText.danger
        ConnectionState.NotConnected -> Brand.lineMuted
    }
    val switchLabel = stringResource(R.string.home_connection)
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .clip(MaterialTheme.shapes.large)
            .background(Brush.linearGradient(listOf(palette.heroStart, palette.heroEnd)))
            .padding(BrandSpacing.lg),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(
                modifier = Modifier.weight(1f),
                verticalArrangement = Arrangement.spacedBy(BrandSpacing.xs),
            ) {
                Text(switchLabel, style = MaterialTheme.typography.labelMedium, color = Brand.lineMuted)
                Row(
                    verticalAlignment = Alignment.CenterVertically,
                    horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs),
                ) {
                    Box(
                        Modifier
                            .size(StatusDot)
                            .clip(CircleShape)
                            .background(dot),
                    )
                    Text(status, style = MaterialTheme.typography.titleLarge, color = Brand.line)
                }
                Text(
                    snapshot?.selectedNode ?: stringResource(R.string.home_no_node),
                    style = MaterialTheme.typography.bodyMedium,
                    color = Brand.lineMuted,
                )
            }
            Switch(
                checked = connected,
                enabled = !busy && snapshot != null,
                onCheckedChange = { on -> if (on) onConnect() else onDisconnect() },
                modifier = Modifier.semantics {
                    contentDescription = switchLabel
                    stateDescription = status
                },
            )
        }
    }
}

@Composable
private fun ErrorCard(message: String) {
    val palette = LocalBrandPalette.current
    Surface(
        shape = MaterialTheme.shapes.medium,
        color = palette.dangerSoft,
        contentColor = palette.text,
        modifier = Modifier.fillMaxWidth(),
    ) {
        Column(
            modifier = Modifier.padding(BrandSpacing.md),
            verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs),
        ) {
            Text(
                stringResource(R.string.home_last_error),
                style = MaterialTheme.typography.labelMedium,
                color = palette.danger,
            )
            Text(message, style = MaterialTheme.typography.bodyMedium, color = palette.text)
        }
    }
}

@Composable
private fun statusLabel(state: ConnectionState): String = stringResource(
    when (state) {
        ConnectionState.Connected -> R.string.status_connected
        ConnectionState.NotConnected -> R.string.status_not_connected
        ConnectionState.Failed -> R.string.status_failed
        ConnectionState.CoreError -> R.string.status_core_error
    },
)

@Composable
private fun routingLabel(mode: String?): String = when (mode) {
    "bypass_china" -> stringResource(R.string.routing_bypass_china)
    "gfw_list" -> stringResource(R.string.routing_gfw_list)
    "return_china" -> stringResource(R.string.routing_return_china)
    "direct" -> stringResource(R.string.routing_direct)
    "rule" -> stringResource(R.string.routing_rule)
    else -> mode.orEmpty()
}

/** A subscription by its host only: the address carries the user's token. */
private fun subscriptionName(url: String): String =
    runCatching { URI(url).host }.getOrNull() ?: url

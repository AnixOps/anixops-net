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
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.anixops.net.R
import io.anixops.net.core.ConnectionSnapshot
import io.anixops.net.core.Overview
import io.anixops.net.core.Traffic
import io.anixops.net.ui.theme.Brand
import io.anixops.net.ui.theme.BrandPalette
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette
import java.net.URI

private val StatusDot = 10.dp

/** The routing modes, in the order the other clients offer them. Public builds ship no preset list modes. */
internal val RoutingModes = listOf("rule", "direct")

/** How many open connections the page lists; the rest are counted in the traffic card. */
private const val ConnectionRows = 20

/** What the overview page can ask for. */
data class HomeActions(
    val connect: () -> Unit = {},
    val disconnect: () -> Unit = {},
    val setRoutingMode: (String) -> Unit = {},
    val closeConnection: (Long) -> Unit = {},
    val closeAll: () -> Unit = {},
)

/** 首页: the connection switch on the brand hero card, live traffic, what the connection uses, open connections. */
@Composable
fun HomeScreen(state: UiState, actions: HomeActions, modifier: Modifier = Modifier) {
    val snapshot = state.snapshot
    val overview = state.overview
    Page(stringResource(R.string.nav_home), modifier) {
        ConnectionCard(overview, state.busy, actions)
        val error = state.notice ?: overview?.error
        if (error != null) ErrorCard(error)
        TrafficSection(overview?.traffic)
        Section(title = null) {
            InfoRow(
                stringResource(R.string.home_egress),
                when {
                    overview?.egress == "direct" -> stringResource(R.string.home_egress_direct)
                    else -> overview?.node?.label ?: stringResource(R.string.home_no_node)
                },
            )
            InfoRow(
                stringResource(R.string.home_subscription),
                snapshot?.subscription?.let(::subscriptionName) ?: stringResource(R.string.home_no_subscription),
                divider = true,
            )
            RoutingRow(snapshot?.routingMode, enabled = snapshot != null && !state.busy, onChoose = actions.setRoutingMode)
            InfoRow(
                stringResource(R.string.home_exit_ip),
                state.probe?.let { probe -> probe.exitIp?.let { listOfNotNull(it, probe.exitLocation).joinToString(" · ") } }
                    ?: stringResource(R.string.home_after_connect),
                divider = true,
            )
        }
        if (overview?.state == "connected") ConnectionsSection(state.connections, actions)
    }
}

/** The hero card (brand moment, E1): always the dark brand gradient, so its colours are the dark palette's. */
@Composable
private fun ConnectionCard(overview: Overview?, busy: Boolean, actions: HomeActions) {
    val palette = LocalBrandPalette.current
    val heroText = BrandPalette.Dark
    val state = overview?.state ?: "disconnected"
    val connected = state == "connected"
    val status = if (busy) stringResource(R.string.status_working) else statusLabel(state)
    val dot = when (state) {
        "connected" -> heroText.success
        "failed", "core_error" -> heroText.danger
        else -> Brand.lineMuted
    }
    val node = overview?.node
    val detail = when {
        overview == null -> ""
        overview.egress == "direct" -> stringResource(R.string.home_egress_direct)
        node != null -> node.label
        else -> stringResource(R.string.home_choose_node)
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
                Text(detail, style = MaterialTheme.typography.bodyMedium, color = Brand.lineMuted)
            }
            Switch(
                checked = connected,
                enabled = !busy && overview != null && (connected || overview.canConnect),
                onCheckedChange = { on -> if (on) actions.connect() else actions.disconnect() },
                modifier = Modifier.semantics {
                    contentDescription = switchLabel
                    stateDescription = status
                },
            )
        }
    }
}

@Composable
private fun TrafficSection(traffic: Traffic?) {
    Section(stringResource(R.string.home_traffic)) {
        if (traffic == null) {
            Text(
                stringResource(R.string.home_after_connect),
                style = MaterialTheme.typography.bodyMedium,
                color = LocalBrandPalette.current.text2,
                modifier = Modifier.padding(BrandSpacing.md),
            )
        } else {
            Row(modifier = Modifier.padding(BrandSpacing.md)) {
                TrafficFigure(stringResource(R.string.home_upload), traffic.uploadRate, traffic.uploadTotal, Modifier.weight(1f))
                TrafficFigure(
                    stringResource(R.string.home_download),
                    traffic.downloadRate,
                    traffic.downloadTotal,
                    Modifier.weight(1f),
                )
            }
        }
    }
}

@Composable
private fun TrafficFigure(label: String, rate: Long, total: Long, modifier: Modifier = Modifier) {
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
        Text(label, style = MaterialTheme.typography.labelMedium, color = LocalBrandPalette.current.text2)
        Text(formatRate(rate), style = MaterialTheme.typography.titleLarge)
        Text(
            stringResource(R.string.home_total, formatBytes(total)),
            style = MaterialTheme.typography.bodySmall,
            color = LocalBrandPalette.current.text2,
        )
    }
}

@Composable
private fun RoutingRow(mode: String?, enabled: Boolean, onChoose: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        InfoRow(
            stringResource(R.string.home_routing),
            routingLabel(mode),
            divider = true,
            onClick = if (enabled) ({ open = true }) else null,
        )
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            RoutingModes.forEach { option ->
                DropdownMenuItem(
                    text = { Text(routingLabel(option)) },
                    onClick = {
                        open = false
                        if (option != mode) onChoose(option)
                    },
                )
            }
        }
    }
}

@Composable
private fun ConnectionsSection(connections: List<ConnectionSnapshot>, actions: HomeActions) {
    Section(
        title = stringResource(R.string.home_connections) + " · " +
            pluralStringResource(R.plurals.home_connection_count, connections.size, connections.size),
        action = {
            if (connections.isNotEmpty()) {
                TextButton(onClick = actions.closeAll) { Text(stringResource(R.string.home_close_all)) }
            }
        },
    ) {
        if (connections.isEmpty()) {
            Text(
                stringResource(R.string.home_no_connections),
                style = MaterialTheme.typography.bodyMedium,
                color = LocalBrandPalette.current.text2,
                modifier = Modifier.padding(BrandSpacing.md),
            )
        }
        // Newest first.
        connections.asReversed().take(ConnectionRows).forEachIndexed { index, connection ->
            if (index > 0) RowDivider()
            ConnectionRow(connection, onClose = { actions.closeConnection(connection.id) })
        }
    }
}

@Composable
private fun ConnectionRow(connection: ConnectionSnapshot, onClose: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = RowMinHeight)
            .padding(start = BrandSpacing.md, top = BrandSpacing.xs, bottom = BrandSpacing.xs),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(modifier = Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
            Text(
                connection.name,
                style = MaterialTheme.typography.bodyLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                listOf(connection.network.uppercase(), connection.via, formatBytes(connection.upload + connection.download))
                    .filter { it.isNotBlank() }
                    .joinToString(" · "),
                style = MaterialTheme.typography.bodySmall,
                color = LocalBrandPalette.current.text2,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        IconButton(onClick = onClose) {
            Icon(
                Icons.Filled.Close,
                contentDescription = stringResource(R.string.home_close_connection, connection.name),
                tint = LocalBrandPalette.current.text3,
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
internal fun statusLabel(state: String): String = stringResource(
    when (state) {
        "connected" -> R.string.status_connected
        "failed" -> R.string.status_failed
        "core_error" -> R.string.status_core_error
        else -> R.string.status_not_connected
    },
)

@Composable
internal fun routingLabel(mode: String?): String = when (mode) {
    "direct" -> stringResource(R.string.routing_direct)
    "rule" -> stringResource(R.string.routing_rule)
    else -> mode.orEmpty()
}

/** A subscription by its host only: the address carries the user's token. */
private fun subscriptionName(url: String): String =
    runCatching { URI(url).host }.getOrNull() ?: url

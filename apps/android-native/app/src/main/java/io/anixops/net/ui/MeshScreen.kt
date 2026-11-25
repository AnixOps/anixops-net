package io.anixops.net.ui

import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.icu.text.ListFormatter
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
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
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.anixops.net.R
import io.anixops.net.core.MeshChild
import io.anixops.net.core.MeshCustomPath
import io.anixops.net.core.MeshEntry
import io.anixops.net.core.MeshGroup
import io.anixops.net.core.MeshLink
import io.anixops.net.core.MeshMemberRow
import io.anixops.net.core.MeshNotice
import io.anixops.net.core.MeshPage
import io.anixops.net.core.MeshSubnetRow
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull

// 组网 (mesh) on Android (docs/mesh-ui.md): the overview's section and the mesh page. The phone is an access endpoint
// only: no relay, no advertising, no host mode. Grouping, folding, searching and the notices are the shared views'
// (view.mesh_*); these composables draw their answers.

internal const val MESH_ENTRY_TAG = "mesh-entry"
internal const val MESH_JOIN_TAG = "mesh-join"
internal const val MESH_ADD_TAG = "mesh-add"
internal const val MESH_REMOVE_TAG = "mesh-remove"
internal const val MESH_ADD_PATH_TAG = "mesh-add-path"

private val Dot = 8.dp

/** 首页's 组网 section: network, state, members online, the encryption mark and the join switch (separate from 连接). */
@Composable
fun MeshEntrySection(entry: MeshEntry, busy: Boolean, actions: MeshActions, open: () -> Unit) {
    val palette = LocalBrandPalette.current
    Section(stringResource(R.string.mesh_title)) {
        Row(
            modifier = Modifier
                .testTag(MESH_ENTRY_TAG)
                .fillMaxWidth()
                .clickable(onClick = open)
                .heightIn(min = 56.dp)
                .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.sm),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
        ) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
                    StateDot(entry.state)
                    Text(
                        entry.network?.let { "$it · ${stateLabel(entry.state)}" } ?: stringResource(R.string.mesh_no_network),
                        style = MaterialTheme.typography.bodyLarge,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                    if (entry.attention) {
                        val attention = stringResource(R.string.mesh_attention)
                        Box(
                            Modifier
                                .size(Dot)
                                .clip(CircleShape)
                                .background(palette.warning)
                                .semantics { contentDescription = attention },
                        )
                    }
                }
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
                    if (entry.state == "joined" || entry.state == "partial") {
                        Text(
                            stringResource(R.string.mesh_online, entry.online.toString(), entry.total.toString()),
                            style = MaterialTheme.typography.bodySmall,
                            color = palette.text2,
                        )
                    }
                    if (entry.security != "forward_secrecy") SecurityTag(entry.security)
                }
            }
            if (entry.network != null) JoinSwitch(entry.state, busy, actions.setJoined)
        }
    }
}

@Composable
private fun JoinSwitch(state: String, busy: Boolean, setJoined: (Boolean) -> Unit) {
    val label = stringResource(R.string.mesh_join)
    val joined = state == "joined" || state == "joining" || state == "partial"
    val status = stateLabel(state)
    Switch(
        checked = joined,
        enabled = !busy,
        onCheckedChange = setJoined,
        modifier = Modifier
            .testTag(MESH_JOIN_TAG)
            .semantics {
                contentDescription = label
                stateDescription = status
            },
    )
}

/** The views of the mesh page. */
enum class MeshTab { Members, Subnets, Paths }

/** The mesh page: the status section, the notices, then 成员 · 子网 · 路径, opening on [initialTab]. */
@Composable
fun MeshScreen(
    state: MeshState,
    actions: MeshActions,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
    initialTab: MeshTab = MeshTab.Members,
) {
    val page = state.page
    var tab by rememberSaveable { mutableStateOf(initialTab) }
    var adding by rememberSaveable { mutableStateOf(false) }
    var removing by rememberSaveable { mutableStateOf<String?>(null) }
    var addingPath by rememberSaveable { mutableStateOf(false) }
    if (addingPath && page != null) {
        MeshPathScreen(
            page = page,
            save = { draft -> actions.addPath(draft).also { saved -> if (saved) addingPath = false } },
            onBack = { addingPath = false },
            modifier = modifier,
        )
        return
    }
    if (adding) {
        MeshNetworkScreen(
            taken = state.saved.map { it.name }.toSet(),
            join = { draft ->
                actions.addNetwork(draft)
                adding = false
            },
            onBack = { adding = false },
            modifier = modifier,
        )
        return
    }
    Page(stringResource(R.string.mesh_title), modifier, onBack = onBack) {
        if (page == null) {
            Note(stringResource(R.string.mesh_unavailable))
            return@Page
        }
        StatusSection(page, state, actions)
        Notices(page.notices)
        val network = page.head.network
        if (network == null) {
            Button(
                onClick = { adding = true },
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 48.dp)
                    .testTag(MESH_ADD_TAG),
            ) { Text(stringResource(R.string.mesh_add_network)) }
            return@Page
        }
        SingleChoiceSegmentedButtonRow(modifier = Modifier.fillMaxWidth()) {
            val tabs = listOf(
                MeshTab.Members to stringResource(R.string.mesh_members),
                MeshTab.Subnets to stringResource(R.string.mesh_subnets),
                MeshTab.Paths to stringResource(R.string.mesh_paths),
            )
            tabs.forEachIndexed { index, (value, label) ->
                SegmentedButton(
                    selected = value == tab,
                    onClick = { tab = value },
                    shape = SegmentedButtonDefaults.itemShape(index, tabs.size),
                ) { Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis) }
            }
        }
        if (tab != MeshTab.Paths) {
            OutlinedTextField(
                value = state.query,
                onValueChange = actions.setQuery,
                singleLine = true,
                label = { Text(stringResource(R.string.mesh_search)) },
                modifier = Modifier.fillMaxWidth(),
            )
        }
        when (tab) {
            MeshTab.Members -> Members(page, state, actions)
            MeshTab.Subnets -> Subnets(page, state, actions)
            MeshTab.Paths -> Paths(page, state, actions) { addingPath = true }
        }
        // 添加网络, then 移除网络 after a divider (docs/mesh-ui.md, 入口与导航).
        Section(null) {
            ActionRow(stringResource(R.string.mesh_add_network), LocalBrandPalette.current.primary, MESH_ADD_TAG) {
                adding = true
            }
            RowDivider()
            ActionRow(stringResource(R.string.mesh_remove_network), LocalBrandPalette.current.danger, MESH_REMOVE_TAG) {
                removing = network.name ?: network.id
            }
        }
    }
    removing?.let { name ->
        AlertDialog(
            onDismissRequest = { removing = null },
            title = { Text(stringResource(R.string.mesh_remove_confirm, name)) },
            text = { Text(stringResource(R.string.mesh_remove_message)) },
            confirmButton = {
                TextButton(onClick = {
                    removing = null
                    actions.removeNetwork()
                }) { Text(stringResource(R.string.mesh_remove_network), color = LocalBrandPalette.current.danger) }
            },
            dismissButton = { TextButton(onClick = { removing = null }) { Text(stringResource(R.string.action_cancel)) } },
        )
    }
}

/** A row of a [Section] that is one action, in [color]. */
@Composable
private fun ActionRow(label: String, color: Color, tag: String, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .heightIn(min = 48.dp)
            .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.sm)
            .testTag(tag),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, style = MaterialTheme.typography.bodyLarge, color = color)
    }
}

@Composable
private fun StatusSection(page: MeshPage, state: MeshState, actions: MeshActions) {
    val network = page.head.network
    if (network == null) {
        Section(null) { InfoRow(stringResource(R.string.mesh_title), stringResource(R.string.mesh_no_network)) }
        Note(stringResource(R.string.mesh_no_network_hint))
        return
    }
    val own = page.head.own
    val context = LocalContext.current
    Section(null) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 56.dp)
                .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.sm),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
                Text(network.name ?: network.id, style = MaterialTheme.typography.titleMedium)
                Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
                    StateDot(network.state)
                    Text(statusLine(page), style = MaterialTheme.typography.bodySmall, color = LocalBrandPalette.current.text2)
                }
            }
            JoinSwitch(network.state, state.busy, actions.setJoined)
        }
        if (securityLabel(network.security) != null) {
            RowDivider()
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = 48.dp)
                    .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.sm),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(stringResource(R.string.mesh_security), style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                SecurityTag(network.security)
            }
        }
        own?.domain?.let { domain ->
            InfoRow(stringResource(R.string.mesh_domain), domain, divider = true, onClick = { copy(context, domain) })
        }
        own?.address?.let { address ->
            InfoRow(stringResource(R.string.mesh_address), address, divider = true, onClick = { copy(context, address) })
        }
        own?.region?.let { code ->
            val name = page.head.regionName(code).orEmpty()
            val basis = own.regionBasis?.border
            InfoRow(
                stringResource(R.string.mesh_region),
                when {
                    own.regionState == "pending" -> stringResource(R.string.mesh_region_pending, name)
                    own.regionState == "pinned" -> stringResource(R.string.mesh_region_pinned, name)
                    basis != null -> stringResource(R.string.mesh_region_basis, name, basis)
                    else -> name
                },
                divider = true,
            )
        }
    }
    state.refusal?.let { refusal -> NoticeCard(failureText(refusal.code), refusal.detail, actions.dismissRefusal) }
}

@Composable
private fun statusLine(page: MeshPage): String {
    val network = page.head.network ?: return ""
    val joining = network.joining
    if (network.state == "joining" && joining != null) {
        if (joining.stage == "connecting" && joining.peersConnected != null) {
            val peers = joining.peersConnected ?: 0
            return pluralStringResource(R.plurals.mesh_joining_peers, peers, peers)
        }
        joiningStage(joining.stage)?.let { return it }
    }
    if (network.state == "joined" || network.state == "partial") {
        return "${stateLabel(network.state)} · ${stringResource(R.string.mesh_online, page.entry.online.toString(), page.entry.total.toString())}"
    }
    return stateLabel(network.state)
}

@Composable
private fun Notices(notices: List<MeshNotice>) {
    notices.forEach { notice -> NoticeCard(noticeText(notice), noticeDetail(notice), dismiss = null) }
}

@Composable
private fun NoticeCard(text: String, detail: String?, dismiss: (() -> Unit)?) {
    val palette = LocalBrandPalette.current
    Surface(shape = MaterialTheme.shapes.medium, color = palette.warningSoft, contentColor = palette.text, modifier = Modifier.fillMaxWidth()) {
        Column(Modifier.padding(BrandSpacing.md), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
            Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(BrandSpacing.sm)) {
                Icon(Icons.Filled.Warning, contentDescription = null, tint = palette.amberText)
                Text(text, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
                if (dismiss != null) {
                    IconButton(onClick = dismiss) {
                        Icon(Icons.Filled.Close, contentDescription = stringResource(R.string.notice_dismiss))
                    }
                }
            }
            if (detail != null) {
                Text(detail, style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace, color = palette.text2)
            }
        }
    }
}

// Members.

private val GroupOptions = listOf("region" to R.string.mesh_group_region, "role" to R.string.mesh_group_role, "none" to R.string.mesh_group_none)
private val FilterOptions = listOf(
    "all" to R.string.mesh_filter_all,
    "online" to R.string.mesh_filter_online,
    "unreachable" to R.string.mesh_unreachable,
    "gateway" to R.string.mesh_role_gateway,
    "host" to R.string.mesh_role_host,
    "border" to R.string.mesh_role_border,
    "custom_path" to R.string.mesh_filter_custom_path,
)
private val SortOptions = listOf("name" to R.string.mesh_sort_name, "latency" to R.string.mesh_sort_latency, "address" to R.string.mesh_address)

@Composable
private fun Members(page: MeshPage, state: MeshState, actions: MeshActions) {
    val members = page.members
    Row(horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
        OptionMenu(R.string.mesh_group, GroupOptions, state.groupBy, actions.setGroupBy)
        OptionMenu(R.string.mesh_filter, FilterOptions, state.filter, actions.setFilter)
        OptionMenu(R.string.mesh_sort, SortOptions, state.sort, actions.setSort)
    }
    if (members.groups.isEmpty()) {
        Note(stringResource(if (state.query.isEmpty() && state.filter == "all") R.string.mesh_no_members else R.string.mesh_no_matching_members))
        return
    }
    val opened = remember { mutableStateMapOf<String, Boolean>() }
    var detail by rememberSaveable { mutableStateOf<String?>(null) }
    members.groups.forEach { group ->
        val key = group.key.orEmpty()
        val open = opened[key] ?: group.open
        val grouped = members.groupBy != "none"
        val title = if (grouped) groupTitle(page, members.groupBy, group) else null
        val fold: (@Composable () -> Unit)? = if (grouped) {
            { TextButton(onClick = { opened[key] = !open }) { Text(stringResource(if (open) R.string.mesh_fold else R.string.mesh_unfold)) } }
        } else {
            null
        }
        Section(title, action = fold) {
            if (open) {
                group.rows.forEachIndexed { index, row ->
                    if (index > 0) RowDivider()
                    MemberRow(page, row, detail == row.id) { detail = if (detail == row.id) null else row.id }
                }
            } else {
                Text(
                    groupSummary(group),
                    style = MaterialTheme.typography.bodySmall,
                    color = LocalBrandPalette.current.text2,
                    modifier = Modifier.padding(BrandSpacing.md),
                )
            }
        }
    }
}

@Composable
private fun groupTitle(page: MeshPage, by: String, group: MeshGroup): String {
    val name = if (by == "role") roleLabel(group.key) ?: group.key.orEmpty() else page.head.regionName(group.key).orEmpty()
    return "$name · ${pluralStringResource(R.plurals.mesh_members_count, group.count, group.count)}"
}

@Composable
private fun groupSummary(group: MeshGroup): String {
    val parts = mutableListOf<String>()
    group.link?.let { link ->
        link.via?.let { parts += stringResource(R.string.mesh_via, it) }
        parts += latencyText(link.latencyMs)
    }
    group.worst?.let { worst ->
        parts += if (worst.state == "unreachable") {
            pluralStringResource(R.plurals.mesh_unreachable_count, worst.count, worst.count)
        } else {
            pluralStringResource(R.plurals.mesh_fallback_count, worst.count, worst.count)
        }
    }
    return parts.joinToString(" · ")
}

/**
 * A member: name, role mark, marks and latency; then the region part of the internal domain name, the connection,
 * the transport and what it contains (N34). Tapping it unfolds its subnets, devices or virtual machines (one level,
 * N30, more than five folded, N31) and the detail.
 */
@Composable
private fun MemberRow(page: MeshPage, row: MeshMemberRow, expanded: Boolean, toggle: () -> Unit) {
    val palette = LocalBrandPalette.current
    val open = expanded || row.open
    var all by remember(row.id) { mutableStateOf(false) }
    Column(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = toggle)
            .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.sm),
        verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
            StateDot(row.state)
            Text(row.name, style = MaterialTheme.typography.bodyLarge, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
            roleLabel(row.role)?.let { Tag(it, palette.primary, palette.soft) }
            if (row.isSelf) Tag(stringResource(R.string.mesh_self), palette.text2, palette.raised)
            row.marks.forEach { mark -> markLabel(mark)?.let { Tag(it, palette.amberText, palette.warningSoft) } }
            Box(Modifier.weight(1f))
            if (!row.isSelf) Latency(row.latencyMs, row.tone)
        }
        Text(secondLine(row), style = MaterialTheme.typography.bodySmall, color = palette.text2, maxLines = 1, overflow = TextOverflow.Ellipsis)
        if (open) {
            val shown = if (all) row.children else row.children.take(row.visibleChildren)
            shown.forEach { child -> ChildLine(child) }
            if (shown.size < row.children.size) {
                TextButton(onClick = { all = true }) {
                    Text(pluralStringResource(R.plurals.list_show_all, row.children.size, row.children.size))
                }
            }
            if (expanded) MemberDetail(page, row)
        }
    }
}

@Composable
private fun secondLine(row: MeshMemberRow): String {
    val parts = mutableListOf<String>()
    row.regionDomain?.let { parts += it }
    parts += connectionLabel(row.connection, row.hops)
    row.transport?.let { parts += it.uppercase() }
    row.contains?.let { contains ->
        parts += if (contains.noun == "vms") {
            pluralStringResource(R.plurals.mesh_vms_count, contains.count, contains.count)
        } else {
            pluralStringResource(R.plurals.mesh_subnets_count, contains.count, contains.count)
        }
    }
    return parts.joinToString(" · ")
}

@Composable
private fun ChildLine(child: MeshChild) {
    Column(Modifier.padding(start = BrandSpacing.md)) {
        Text(
            child.name,
            style = MaterialTheme.typography.bodyMedium,
            fontWeight = if (child.match) FontWeight.SemiBold else FontWeight.Normal,
            fontFamily = if (child.kind == "subnet") FontFamily.Monospace else FontFamily.Default,
        )
        (child.domain ?: child.address)?.let {
            Text(it, style = MaterialTheme.typography.bodySmall, color = LocalBrandPalette.current.text2, fontFamily = FontFamily.Monospace)
        }
    }
}

/** The detail under an unfolded member: internal domain name and address (both copyable), region, path. */
@Composable
private fun MemberDetail(page: MeshPage, row: MeshMemberRow) {
    val context = LocalContext.current
    val palette = LocalBrandPalette.current
    Column(Modifier.padding(top = BrandSpacing.xs), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
        row.domain?.let { domain ->
            DetailLine(stringResource(R.string.mesh_domain), domain) { copy(context, domain) }
        }
        row.address?.let { address ->
            DetailLine(stringResource(R.string.mesh_address), address) { copy(context, address) }
        }
        row.region?.let { DetailLine(stringResource(R.string.mesh_region), page.head.regionName(it).orEmpty(), null) }
        if (row.marks.contains("duplicate")) {
            Text(stringResource(R.string.mesh_duplicate_detail), style = MaterialTheme.typography.bodySmall, color = palette.amberText)
        }
        if (row.marks.contains("unmanaged")) {
            Text(stringResource(R.string.mesh_unmanaged_detail), style = MaterialTheme.typography.bodySmall, color = palette.amberText)
        }
    }
}

@Composable
private fun DetailLine(label: String, value: String, copy: (() -> Unit)?) {
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(BrandSpacing.sm)) {
        Text(label, style = MaterialTheme.typography.bodySmall, color = LocalBrandPalette.current.text2)
        Text(value, style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace, modifier = Modifier.weight(1f))
        if (copy != null) TextButton(onClick = copy) { Text(stringResource(R.string.action_copy)) }
    }
}

// Subnets.

@Composable
private fun Subnets(page: MeshPage, state: MeshState, actions: MeshActions) {
    val subnets = page.subnets
    if (subnets.rows.isEmpty()) {
        Note(stringResource(if (state.query.isEmpty()) R.string.mesh_no_subnets else R.string.mesh_no_matching_subnets))
        return
    }
    Section(null) {
        subnets.rows.forEachIndexed { index, row ->
            if (index > 0) RowDivider()
            SubnetRow(row, state.busy, actions)
        }
    }
    if (subnets.refused > 0) Note(pluralStringResource(R.plurals.mesh_refused_count, subnets.refused, subnets.refused))
}

@Composable
private fun SubnetRow(row: MeshSubnetRow, busy: Boolean, actions: MeshActions) {
    val palette = LocalBrandPalette.current
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.sm),
        verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
            StateDot(if (row.state == "ok") "online" else "unreachable")
            Text(row.cidr, style = MaterialTheme.typography.bodyLarge, fontFamily = FontFamily.Monospace)
            row.marks.forEach { mark -> markLabel(mark)?.let { Tag(it, palette.amberText, palette.warningSoft) } }
            Box(Modifier.weight(1f))
            if (row.state != "refused") Latency(row.latencyMs, row.tone)
        }
        val detail = if (row.state == "refused") {
            stringResource(R.string.mesh_subnet_refused)
        } else {
            listOfNotNull(row.advertiser, row.regionName, row.path.takeIf { it.isNotEmpty() }?.joinToString(" → ")).joinToString(" · ")
        }
        Text(detail, style = MaterialTheme.typography.bodySmall, color = palette.text2)
        if (row.advertisers.size > 1) {
            row.advertisers.forEach { advertiser ->
                Text(
                    "${advertiser.name} · ${stringResource(if (advertiser.inUse) R.string.mesh_in_use else R.string.mesh_standby)}",
                    style = MaterialTheme.typography.bodySmall,
                    color = palette.text2,
                )
            }
        }
        if (row.overlaps.isNotEmpty()) {
            Text(
                stringResource(R.string.mesh_overlaps_with, ListFormatter.getInstance().format(row.overlaps)),
                style = MaterialTheme.typography.bodySmall,
                color = palette.amberText,
            )
        }
        if ("local_overlap" in row.marks) {
            SwitchRow(stringResource(R.string.mesh_route_through), null, row.route == "mesh", !busy) { on ->
                actions.setSubnetRoute(row.cidr, if (on) "mesh" else "local")
            }
        }
    }
}

// Paths.

@Composable
private fun Paths(page: MeshPage, state: MeshState, actions: MeshActions, addPath: () -> Unit) {
    val paths = page.paths
    Section(stringResource(R.string.mesh_default_routing)) {
        Column(Modifier.padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.sm)) {
            Text(stringResource(R.string.mesh_policy), style = MaterialTheme.typography.bodyLarge)
            SingleChoiceSegmentedButtonRow(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(top = BrandSpacing.xs),
            ) {
                val options = listOf(
                    "latency" to stringResource(R.string.mesh_policy_latency),
                    "hops" to stringResource(R.string.mesh_policy_hops),
                )
                val selected = if (paths.policy == "hops") "hops" else "latency"
                options.forEachIndexed { index, (value, label) ->
                    SegmentedButton(
                        selected = value == selected,
                        onClick = { if (value != selected) actions.setPolicy(value) },
                        enabled = !state.busy,
                        shape = SegmentedButtonDefaults.itemShape(index, options.size),
                    ) { Text(label, maxLines = 1, overflow = TextOverflow.Ellipsis) }
                }
            }
        }
    }
    paths.links?.let { links ->
        Section(stringResource(R.string.mesh_links)) {
            if (links.isEmpty()) Note(stringResource(R.string.mesh_no_links))
            links.forEachIndexed { index, link ->
                if (index > 0) RowDivider()
                LinkRow(link)
            }
        }
    }
    paths.custom?.let { custom ->
        Section(stringResource(R.string.mesh_custom_paths)) {
            if (custom.isEmpty()) Note(stringResource(R.string.mesh_no_custom_paths))
            custom.forEachIndexed { index, path ->
                if (index > 0) RowDivider()
                CustomPathRow(path, !state.busy) { actions.removePath(path.id) }
            }
            if (state.removedPaths != null) {
                RowDivider()
                Row(
                    Modifier
                        .fillMaxWidth()
                        .padding(start = BrandSpacing.md, end = BrandSpacing.xs),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(
                        stringResource(R.string.mesh_path_removed),
                        style = MaterialTheme.typography.bodyMedium,
                        color = LocalBrandPalette.current.text2,
                        modifier = Modifier.weight(1f),
                    )
                    TextButton(onClick = actions.undoPathRemoval, enabled = !state.busy) {
                        Text(stringResource(R.string.mesh_undo))
                    }
                }
            }
            RowDivider()
            ActionRow(stringResource(R.string.mesh_add_path), LocalBrandPalette.current.primary, MESH_ADD_PATH_TAG, addPath)
        }
    }
}

@Composable
private fun LinkRow(link: MeshLink) {
    val parts = mutableListOf<String>()
    parts += when (link.state) {
        "detour" -> stringResource(R.string.mesh_link_detour, link.viaName.orEmpty())
        "down" -> stringResource(R.string.mesh_link_down)
        else -> stringResource(R.string.mesh_connection_direct)
    }
    if (link.borders.isNotEmpty()) parts += ListFormatter.getInstance(LocalConfiguration.current.locales[0]).format(link.borders)
    link.loss?.let { parts += stringResource(R.string.mesh_loss, Math.round(it * 100).toString()) }
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.sm),
        verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
            StateDot(
                when (link.state) {
                    "down" -> "unreachable"
                    "detour" -> "partial"
                    else -> "online"
                },
            )
            Text("${link.fromName} – ${link.toName}", style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            Latency(link.latencyMs, link.tone)
        }
        Text(parts.joinToString(" · "), style = MaterialTheme.typography.bodySmall, color = LocalBrandPalette.current.text2)
    }
}

@Composable
private fun CustomPathRow(path: MeshCustomPath, enabled: Boolean, remove: () -> Unit) {
    val palette = LocalBrandPalette.current
    val parts = mutableListOf<String>()
    if (path.via.isNotEmpty()) parts += stringResource(R.string.mesh_via, path.via.joinToString(" → "))
    path.advertiser?.let { parts += stringResource(R.string.mesh_advertised_by, it) }
    if (path.state == "fallback") {
        when (path.reason) {
            "unsupported", "not_relay" -> path.unreachable?.let { parts += stringResource(R.string.mesh_member_no_forward, it) }
            "hop_limit" -> parts += stringResource(R.string.mesh_hop_limit)
            else -> path.unreachable?.let { parts += stringResource(R.string.mesh_member_unreachable, it) }
        }
    }
    Column(
        Modifier
            .fillMaxWidth()
            .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.sm),
        verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
            Text(path.targetLabel, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            Text(
                stringResource(
                    when (path.state) {
                        "fallback" -> R.string.mesh_fallen_back
                        "first_hop_only" -> R.string.mesh_first_hop_only
                        "pending" -> R.string.mesh_path_pending
                        else -> R.string.mesh_path_active
                    },
                ),
                style = MaterialTheme.typography.labelMedium,
                color = if (path.state == "active" || path.state == "pending") palette.text2 else palette.amberText,
            )
            val removeLabel = stringResource(R.string.mesh_remove_path, path.targetLabel)
            IconButton(onClick = remove, enabled = enabled, modifier = Modifier.semantics { contentDescription = removeLabel }) {
                Icon(Icons.Filled.Close, contentDescription = null, tint = palette.text2)
            }
        }
        Text(parts.joinToString(" · "), style = MaterialTheme.typography.bodySmall, color = palette.text2)
    }
}

// Pieces.

@Composable
private fun OptionMenu(label: Int, options: List<Pair<String, Int>>, selected: String, onChoose: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    val current = options.firstOrNull { it.first == selected }?.second ?: options.first().second
    Box {
        TextButton(onClick = { open = true }) { Text("${stringResource(label)}: ${stringResource(current)}") }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            options.forEach { (value, name) ->
                DropdownMenuItem(
                    text = { Text(stringResource(name)) },
                    onClick = {
                        open = false
                        onChoose(value)
                    },
                )
            }
        }
    }
}

@Composable
private fun StateDot(state: String?) {
    val palette = LocalBrandPalette.current
    val color = when (state) {
        "joined", "online" -> palette.success
        "joining" -> palette.info
        "partial" -> palette.warning
        "failed", "unreachable" -> palette.danger
        else -> palette.text3
    }
    Box(
        Modifier
            .size(Dot)
            .clip(CircleShape)
            .background(color),
    )
}

@Composable
private fun Tag(text: String, foreground: Color, background: Color) {
    Text(
        text,
        style = MaterialTheme.typography.labelSmall,
        color = foreground,
        maxLines = 1,
        modifier = Modifier
            .clip(MaterialTheme.shapes.extraSmall)
            .background(background)
            .padding(horizontal = 6.dp, vertical = 2.dp),
    )
}

/** 未加密 in the danger colour, 未启用前向保密 in the warning colour; 前向保密 in the accent. */
@Composable
private fun SecurityTag(security: String?) {
    val label = securityLabel(security) ?: return
    val palette = LocalBrandPalette.current
    when (security) {
        "none" -> Tag(label, palette.danger, palette.dangerSoft)
        "standard" -> Tag(label, palette.amberText, palette.warningSoft)
        else -> Tag(label, palette.primary, palette.soft)
    }
}

/** The latency with a dot for its tone (120 and 250 ms, as the node list); the number is not coloured. */
@Composable
private fun Latency(milliseconds: Long?, tone: String?) {
    val palette = LocalBrandPalette.current
    val color = when (tone) {
        "fast" -> palette.success
        "medium" -> palette.warning
        "slow" -> palette.danger
        else -> palette.text3
    }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        Box(
            Modifier
                .size(6.dp)
                .clip(CircleShape)
                .background(color),
        )
        Text(latencyText(milliseconds), style = MaterialTheme.typography.bodySmall, color = palette.text2)
    }
}

@Composable
private fun latencyText(milliseconds: Long?): String =
    milliseconds?.let { stringResource(R.string.mesh_latency_ms, it.toString()) } ?: stringResource(R.string.mesh_timeout)

// Words (docs/mesh-ui.md, 用词 and 状态).

@Composable
private fun stateLabel(state: String?): String = stringResource(
    when (state) {
        "joined" -> R.string.mesh_state_joined
        "joining" -> R.string.mesh_state_joining
        "partial" -> R.string.mesh_state_partial
        "failed" -> R.string.mesh_state_failed
        else -> R.string.mesh_state_not_joined
    },
)

@Composable
private fun securityLabel(security: String?): String? = when (security) {
    "forward_secrecy" -> stringResource(R.string.mesh_security_forward)
    "standard" -> stringResource(R.string.mesh_security_standard)
    "none" -> stringResource(R.string.mesh_security_none)
    else -> null
}

@Composable
private fun roleLabel(role: String?): String? = when (role) {
    "border" -> stringResource(R.string.mesh_role_border)
    "gateway" -> stringResource(R.string.mesh_role_gateway)
    "host" -> stringResource(R.string.mesh_role_host)
    "server" -> stringResource(R.string.mesh_role_server)
    "client" -> stringResource(R.string.mesh_role_client)
    else -> null
}

@Composable
private fun connectionLabel(connection: String, hops: Int): String = when (connection) {
    "self" -> stringResource(R.string.mesh_self)
    "direct" -> stringResource(R.string.mesh_connection_direct)
    "relay" -> pluralStringResource(R.plurals.mesh_connection_relay, hops, hops)
    "cross_region" -> pluralStringResource(R.plurals.mesh_connection_cross_region, hops, hops)
    "via_host" -> stringResource(R.string.mesh_connection_via_host)
    "via_gateway" -> stringResource(R.string.mesh_connection_via_gateway)
    else -> stringResource(R.string.mesh_unreachable)
}

@Composable
private fun markLabel(mark: String): String? = when (mark) {
    "duplicate" -> stringResource(R.string.mesh_mark_duplicate)
    "unmanaged" -> stringResource(R.string.mesh_mark_unmanaged)
    "fallback" -> stringResource(R.string.mesh_fallen_back)
    "first_hop_only" -> stringResource(R.string.mesh_first_hop_only)
    "multi_source" -> stringResource(R.string.mesh_mark_multi_source)
    "overlap" -> stringResource(R.string.mesh_mark_overlap)
    "local_overlap" -> stringResource(R.string.mesh_mark_local_overlap)
    "refused" -> stringResource(R.string.mesh_mark_refused)
    else -> null
}

@Composable
private fun joiningStage(stage: String?): String? = when (stage) {
    "connecting" -> stringResource(R.string.mesh_stage_connecting)
    "assigning_address" -> stringResource(R.string.mesh_stage_address)
    "measuring_regions" -> stringResource(R.string.mesh_stage_regions)
    else -> null
}

@Composable
private fun failureText(code: String?): String = stringResource(
    when (code) {
        "unencrypted_refused" -> R.string.mesh_failure_unencrypted
        "secret_mismatch" -> R.string.mesh_failure_secret
        "security_mismatch" -> R.string.mesh_failure_security
        "port_in_use" -> R.string.mesh_failure_port
        "tun_in_use" -> R.string.mesh_failure_tun
        "route_overlap" -> R.string.mesh_failure_route
        "address_in_use" -> R.string.mesh_failure_address
        "permission_denied" -> R.string.mesh_failure_permission
        "identity_online" -> R.string.mesh_failure_identity
        "existing_program" -> R.string.mesh_failure_existing
        "unreachable_peers" -> R.string.mesh_failure_peers
        "peer_not_native" -> R.string.mesh_failure_not_native
        "native_keys_pending" -> R.string.mesh_failure_keys_pending
        "unsupported" -> R.string.mesh_failure_unsupported
        "invalid_argument" -> R.string.mesh_failure_invalid
        "not_found" -> R.string.mesh_failure_not_found
        "not_allowed" -> R.string.mesh_failure_not_allowed
        else -> R.string.mesh_failure_other
    },
)

@Composable
private fun noticeText(notice: MeshNotice): String {
    fun text(index: Int): String = (notice.values.getOrNull(index) as? JsonPrimitive)?.contentOrNull.orEmpty()
    fun number(index: Int): Int? = (notice.values.getOrNull(index) as? JsonPrimitive)?.intOrNull
    return when (notice.id) {
        "mesh.notice.path_fallback" -> stringResource(R.string.mesh_notice_path_fallback, text(0), text(1))
        "mesh.notice.path_unsupported" -> stringResource(R.string.mesh_notice_path_unsupported, text(0), text(1))
        "mesh.notice.path_hop_limit" -> stringResource(R.string.mesh_notice_path_hop_limit, text(0))
        "mesh.notice.detour_loss" -> stringResource(R.string.mesh_notice_detour_loss, text(0), text(1), text(2), (number(3) ?: 0).toString())
        "mesh.notice.detour_latency" -> number(3)?.let { stringResource(R.string.mesh_notice_detour_latency, text(0), text(1), text(2), it.toString()) }
            ?: stringResource(R.string.mesh_notice_detour_slow, text(0), text(1), text(2))
        "mesh.notice.partial" -> {
            val names = (notice.values.getOrNull(1) as? JsonArray)?.mapNotNull { (it as? JsonPrimitive)?.contentOrNull }.orEmpty()
            pluralStringResource(R.plurals.mesh_notice_partial, names.size, names.size, ListFormatter.getInstance().format(names))
        }
        else -> failureText(notice.key.removePrefix("failure:").substringBefore(':').takeIf { notice.key.startsWith("failure:") })
    }
}

/** The raw detail of a failure (the port, the overlapping network). */
private fun noticeDetail(notice: MeshNotice): String? =
    if (notice.key.startsWith("failure:")) (notice.values.firstOrNull() as? JsonPrimitive)?.contentOrNull else null

private fun copy(context: Context, text: String) {
    context.getSystemService(ClipboardManager::class.java)?.setPrimaryClip(ClipData.newPlainText(text, text))
}

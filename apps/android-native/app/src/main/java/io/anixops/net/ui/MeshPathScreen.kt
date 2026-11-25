package io.anixops.net.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.KeyboardArrowUp
import androidx.compose.material3.Button
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.anixops.net.R
import io.anixops.net.core.MeshPage
import io.anixops.net.core.MeshRouting
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull

internal const val MESH_PATH_SAVE_TAG = "mesh-path-save"

/**
 * 添加路径 on phones (docs/mesh-ui.md, 路径; kernel 0.5): target kind, target, waypoints in order, and the path they
 * give. A path to a subnet names who advertises it. [save] answers false when a member on the path has no virtual
 * address.
 */
@Composable
fun MeshPathScreen(
    page: MeshPage,
    save: (MeshRouting.Draft) -> Boolean,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    val members = remember(page.status) { members(page) }
    val subnets = page.subnets.rows.filter { it.state != "refused" }
    var member by rememberSaveable { mutableStateOf(true) }
    var target by rememberSaveable { mutableStateOf("") }
    val via = remember { mutableStateListOf<String>() }
    var problem by rememberSaveable { mutableStateOf<Int?>(null) }
    fun name(id: String) = members.firstOrNull { it.first == id }?.second ?: id
    BackTo(onBack)
    Page(stringResource(R.string.mesh_add_path), modifier, onBack = onBack) {
        Text(stringResource(R.string.mesh_path_target_kind))
        SingleChoiceSegmentedButtonRow(modifier = Modifier.fillMaxWidth()) {
            val options = listOf(
                true to stringResource(R.string.mesh_members),
                false to stringResource(R.string.mesh_subnets),
            )
            options.forEachIndexed { index, (value, label) ->
                SegmentedButton(
                    selected = value == member,
                    onClick = {
                        if (value != member) {
                            member = value
                            target = ""
                        }
                    },
                    shape = SegmentedButtonDefaults.itemShape(index, options.size),
                ) { Text(label) }
            }
        }
        val targets = if (member) members else subnets.map { it.cidr to it.cidr }
        Picker(
            label = stringResource(R.string.mesh_path_target),
            value = targets.firstOrNull { it.first == target }?.second ?: stringResource(R.string.mesh_path_choose_target),
            options = targets,
        ) { chosen ->
            target = chosen
            via.remove(chosen)
        }
        if (!member) {
            subnets.firstOrNull { it.cidr == target }?.advertiser?.let { Note(stringResource(R.string.mesh_advertised_by, it)) }
        }
        Section(stringResource(R.string.mesh_waypoints)) {
            via.forEachIndexed { index, id ->
                if (index > 0) RowDivider()
                Row(
                    Modifier
                        .fillMaxWidth()
                        .padding(start = BrandSpacing.md, end = BrandSpacing.xxs),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(name(id), style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                    val moveLabel = stringResource(R.string.mesh_move_up, name(id))
                    IconButton(
                        onClick = { via.add(index - 1, via.removeAt(index)) },
                        enabled = index > 0,
                        modifier = Modifier.semantics { contentDescription = moveLabel },
                    ) { Icon(Icons.Filled.KeyboardArrowUp, contentDescription = null) }
                    val removeLabel = stringResource(R.string.mesh_remove_waypoint, name(id))
                    IconButton(
                        onClick = { via.removeAt(index) },
                        modifier = Modifier.semantics { contentDescription = removeLabel },
                    ) { Icon(Icons.Filled.Close, contentDescription = null) }
                }
            }
            if (via.isNotEmpty()) RowDivider()
            Box(Modifier.padding(horizontal = BrandSpacing.xs)) {
                Picker(
                    label = null,
                    value = stringResource(R.string.mesh_add_waypoint),
                    options = members.filter { it.first != target },
                    enabled = via.size < MeshRouting.MAX_WAYPOINTS,
                ) { chosen -> via.add(chosen) }
            }
        }
        val end = if (member) target.takeIf { it.isNotEmpty() }?.let(::name) else target.takeIf { it.isNotEmpty() }
        Note((listOf(stringResource(R.string.mesh_self)) + via.map(::name) + listOfNotNull(end)).joinToString(" → "))
        problem?.let { Text(stringResource(it), color = LocalBrandPalette.current.danger) }
        Button(
            onClick = {
                problem = when {
                    target.isEmpty() -> R.string.mesh_path_problem_target
                    via.isEmpty() -> R.string.mesh_path_problem_via
                    !save(MeshRouting.Draft(member, target, via.toList())) -> R.string.mesh_path_problem_address
                    else -> null
                }
            },
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 48.dp)
                .testTag(MESH_PATH_SAVE_TAG),
        ) { Text(stringResource(R.string.action_save)) }
    }
}

/** A choice from [options] (value to label) in a menu, shown as an outlined button with its [label] and [value]. */
@Composable
private fun Picker(
    label: String?,
    value: String,
    options: List<Pair<String, String>>,
    enabled: Boolean = true,
    onChoose: (String) -> Unit,
) {
    var open by remember { mutableStateOf(false) }
    Box {
        OutlinedButton(
            onClick = { open = true },
            enabled = enabled && options.isNotEmpty(),
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 48.dp),
        ) {
            Row(horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs), modifier = Modifier.fillMaxWidth()) {
                label?.let { Text(it) }
                Text(value, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
            }
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            options.forEach { (id, name) ->
                DropdownMenuItem(
                    text = { Text(name) },
                    onClick = {
                        open = false
                        onChoose(id)
                    },
                )
            }
        }
    }
}

/** Members other than this device: id to name. */
private fun members(page: MeshPage): List<Pair<String, String>> =
    ((page.status as? JsonObject)?.get("members") as? JsonArray).orEmpty().mapNotNull { row ->
        val fields = row as? JsonObject ?: return@mapNotNull null
        val id = (fields["id"] as? JsonPrimitive)?.contentOrNull ?: return@mapNotNull null
        id to ((fields["name"] as? JsonPrimitive)?.contentOrNull ?: id)
    }

package io.anixops.net.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import io.anixops.net.R
import io.anixops.net.core.MeshNetworkDraft
import io.anixops.net.ui.theme.LocalBrandPalette

internal const val MESH_ADD_JOIN_TAG = "mesh-add-join"

/**
 * 添加网络 (docs/mesh-ui.md): name, secret, peers, virtual address and encryption; the region is assigned by the
 * network. The primary action 加入 spans the width at the end (P3). [taken] are the saved networks' names, which a
 * new network may not reuse.
 */
@Composable
fun MeshNetworkScreen(
    taken: Set<String>,
    join: (MeshNetworkDraft) -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    var name by rememberSaveable { mutableStateOf("") }
    var secret by rememberSaveable { mutableStateOf("") }
    var peers by rememberSaveable { mutableStateOf("") }
    var address by rememberSaveable { mutableStateOf("") }
    var forwardSecrecy by rememberSaveable { mutableStateOf(true) }
    var shownProblem by rememberSaveable { mutableStateOf(false) }
    val draft = MeshNetworkDraft(name, secret, peers, address, forwardSecrecy)
    val duplicate = draft.networkName in taken
    BackTo(onBack)
    Page(stringResource(R.string.mesh_add_network), modifier, onBack = onBack) {
        OutlinedTextField(
            value = name,
            onValueChange = { name = it },
            singleLine = true,
            label = { Text(stringResource(R.string.mesh_add_name)) },
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = secret,
            onValueChange = { secret = it },
            singleLine = true,
            label = { Text(stringResource(R.string.mesh_add_secret)) },
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = peers,
            onValueChange = { peers = it },
            minLines = 2,
            label = { Text(stringResource(R.string.mesh_add_peers)) },
            placeholder = { Text(PEER_EXAMPLE) },
            supportingText = { Text(stringResource(R.string.mesh_add_peers_hint)) },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
            modifier = Modifier.fillMaxWidth(),
        )
        OutlinedTextField(
            value = address,
            onValueChange = { address = it },
            singleLine = true,
            label = { Text(stringResource(R.string.mesh_address)) },
            placeholder = { Text(stringResource(R.string.mesh_add_address_auto)) },
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
            modifier = Modifier.fillMaxWidth(),
        )
        Text(stringResource(R.string.mesh_security))
        SingleChoiceSegmentedButtonRow(modifier = Modifier.fillMaxWidth()) {
            val options = listOf(
                true to stringResource(R.string.mesh_security_forward),
                false to stringResource(R.string.mesh_add_standard),
            )
            options.forEachIndexed { index, (value, label) ->
                SegmentedButton(
                    selected = value == forwardSecrecy,
                    onClick = { forwardSecrecy = value },
                    shape = SegmentedButtonDefaults.itemShape(index, options.size),
                ) { Text(label) }
            }
        }
        if (!forwardSecrecy) Note(stringResource(R.string.mesh_add_standard_note))
        val problem = when {
            !shownProblem -> null
            draft.problem != null -> problemText(draft.problem)
            duplicate -> stringResource(R.string.mesh_add_duplicate)
            else -> null
        }
        problem?.let { Text(it, color = LocalBrandPalette.current.danger) }
        Button(
            onClick = {
                if (draft.problem != null || duplicate) {
                    shownProblem = true
                } else {
                    join(draft)
                }
            },
            modifier = Modifier
                .fillMaxWidth()
                .heightIn(min = 48.dp)
                .testTag(MESH_ADD_JOIN_TAG),
        ) { Text(stringResource(R.string.mesh_join)) }
    }
}

private const val PEER_EXAMPLE = "203.0.113.10:11010"

@Composable
private fun problemText(problem: MeshNetworkDraft.Problem?): String? = when (problem) {
    null -> null
    MeshNetworkDraft.Problem.Name -> stringResource(R.string.mesh_add_problem_name)
    MeshNetworkDraft.Problem.Secret -> stringResource(R.string.mesh_add_problem_secret)
    MeshNetworkDraft.Problem.Peers -> stringResource(R.string.mesh_add_problem_peers)
    is MeshNetworkDraft.Problem.Peer -> stringResource(R.string.mesh_add_problem_peer, problem.line)
    MeshNetworkDraft.Problem.Address -> stringResource(R.string.mesh_add_problem_address)
}

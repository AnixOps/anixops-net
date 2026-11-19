package io.anixops.net.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import io.anixops.net.R
import io.anixops.net.core.DataSource
import io.anixops.net.core.RouteTestAnswer
import io.anixops.net.core.RuleHit
import io.anixops.net.core.RuleHitsView
import io.anixops.net.core.dataName
import io.anixops.net.core.dataRule
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette

/**
 * The GEOIP and IP-ASN data lists: only the mainland China lists are built in, so `GEOIP,US` or `IP-ASN,13335` need
 * a list the user adds by URL. Lists the running rules use and nobody provides come first, so a rule that matches
 * nothing is never silent.
 */
@Composable
internal fun DataListsSection(
    missing: List<String>,
    sources: List<DataSource>,
    enabled: Boolean,
    onSave: (name: String, url: String) -> Unit,
    onRemove: (String) -> Unit,
) {
    val palette = LocalBrandPalette.current
    var name by rememberSaveable { mutableStateOf("") }
    var url by rememberSaveable { mutableStateOf("") }
    val added = sources.map { it.name }.toSet()
    val lacking = missing.filter { it !in added }
    val target = dataName(name)
    Column {
        Section(stringResource(R.string.rules_data)) {
            lacking.forEachIndexed { index, item ->
                if (index > 0) RowDivider()
                ToolRow(
                    stringResource(R.string.rules_data_missing, dataRule(item)),
                    stringResource(R.string.rules_data_missing_hint),
                    warning = true,
                ) {
                    TextButton(onClick = { name = item }, enabled = enabled) { Text(stringResource(R.string.rules_data_add_list)) }
                }
            }
            sources.forEachIndexed { index, source ->
                if (index > 0 || lacking.isNotEmpty()) RowDivider()
                ToolRow(dataRule(source.name), source.url) {
                    TextButton(onClick = { onRemove(source.name) }, enabled = enabled) {
                        Text(stringResource(R.string.action_remove))
                    }
                }
            }
            if (lacking.isNotEmpty() || sources.isNotEmpty()) RowDivider()
            Column(
                modifier = Modifier.padding(BrandSpacing.md),
                verticalArrangement = Arrangement.spacedBy(BrandSpacing.xs),
            ) {
                Field(name, { name = it }, stringResource(R.string.rules_data_name), "US, AS13335")
                if (name.isNotBlank() && target == null) {
                    Text(
                        stringResource(R.string.rules_data_name_invalid),
                        style = MaterialTheme.typography.bodySmall,
                        color = palette.warning,
                    )
                }
                Field(url, { url = it }, stringResource(R.string.rules_data_url), "https://", KeyboardType.Uri)
                Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                    Button(
                        onClick = {
                            target?.let { onSave(it, url) }
                            name = ""
                            url = ""
                        },
                        enabled = enabled && target != null && url.trim().startsWith("https://"),
                    ) { Text(stringResource(R.string.action_add)) }
                }
            }
        }
        Note(stringResource(R.string.rules_data_note))
    }
}

/** Asks the running engine which rule decides a connection and where it goes, without opening one. */
@Composable
internal fun RuleTestSection(
    answer: RouteTestAnswer?,
    enabled: Boolean,
    onTest: (host: String, port: Int, network: String, address: String, process: String) -> Unit,
) {
    val palette = LocalBrandPalette.current
    var host by rememberSaveable { mutableStateOf("") }
    var port by rememberSaveable { mutableStateOf("443") }
    var udp by rememberSaveable { mutableStateOf(false) }
    var address by rememberSaveable { mutableStateOf("") }
    Column {
        Section(stringResource(R.string.rules_test)) {
            Column(
                modifier = Modifier.padding(BrandSpacing.md),
                verticalArrangement = Arrangement.spacedBy(BrandSpacing.xs),
            ) {
                Field(host, { host = it }, stringResource(R.string.rules_test_host), "example.com", KeyboardType.Uri)
                Field(port, { port = it.filter(Char::isDigit).take(5) }, stringResource(R.string.rules_test_port), "443", KeyboardType.Number)
                Field(address, { address = it }, stringResource(R.string.rules_test_address), "", KeyboardType.Uri)
            }
            SwitchRow("UDP", null, checked = udp, enabled = enabled, onChange = { udp = it })
            Row(
                Modifier.fillMaxWidth().padding(start = BrandSpacing.md, end = BrandSpacing.md, bottom = BrandSpacing.sm),
                horizontalArrangement = Arrangement.End,
            ) {
                Button(
                    onClick = { onTest(host, port.toIntOrNull() ?: 443, if (udp) "udp" else "tcp", address, "") },
                    enabled = enabled && host.isNotBlank(),
                ) { Text(stringResource(R.string.rules_test_run)) }
            }
            if (answer != null) {
                RowDivider()
                ToolRow(answer.test.rule, stringResource(R.string.rules_test_rule))
                RowDivider()
                ToolRow(answer.test.path, stringResource(R.string.rules_test_exit))
                if (answer.test.needsAddress) {
                    RowDivider()
                    ToolRow(stringResource(R.string.rules_test_needs_address), null, warning = true)
                }
                if (answer.mode != "rule") {
                    RowDivider()
                    ToolRow(
                        stringResource(if (answer.mode == "global") R.string.rules_test_global else R.string.rules_test_direct),
                        null,
                        warning = true,
                    )
                }
            }
        }
        Text(
            stringResource(R.string.rules_test_note),
            style = MaterialTheme.typography.bodySmall,
            color = palette.text2,
            modifier = Modifier.padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.xs),
        )
    }
}

/** How often each rule decided a connection since the rules were loaded, busiest first. */
@Composable
internal fun RuleHitsSection(view: RuleHitsView, enabled: Boolean, onClear: () -> Unit) {
    val busiest = view.busiest()
    Column {
        Section(
            stringResource(R.string.rules_hits),
            action = { TextButton(onClick = onClear, enabled = enabled) { Text(stringResource(R.string.rules_hits_clear)) } },
        ) {
            if (busiest.isEmpty() && view.finalRule.hits == 0L) {
                ToolRow(stringResource(R.string.rules_hits_none), null)
                RowDivider()
            }
            busiest.forEach { rule ->
                HitRow(rule)
                RowDivider()
            }
            HitRow(view.finalRule)
            if (view.unused > 0) {
                RowDivider()
                ToolRow(stringResource(R.string.rules_hits_unused, view.unused), null)
            }
        }
        Note(stringResource(R.string.rules_hits_note))
    }
}

@Composable
private fun HitRow(rule: RuleHit) {
    ToolRow(rule.rule, rule.target) {
        Text(
            rule.hits.toString(),
            style = MaterialTheme.typography.bodyLarge,
            color = LocalBrandPalette.current.text2,
        )
    }
}

/** PROCESS-NAME rules match nothing through the Android VPN, which cannot tell which app opened a connection. */
@Composable
internal fun ProcessRulesWarning() {
    Text(
        stringResource(R.string.rules_process_not_effective),
        style = MaterialTheme.typography.bodySmall,
        color = LocalBrandPalette.current.warning,
        modifier = Modifier.padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.xs),
    )
}

@Composable
private fun ToolRow(label: String, hint: String?, warning: Boolean = false, trailing: (@Composable () -> Unit)? = null) {
    val palette = LocalBrandPalette.current
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = RowMinHeight)
            .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.xs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
            Text(
                label,
                style = MaterialTheme.typography.bodyLarge,
                color = if (warning) palette.warning else MaterialTheme.colorScheme.onSurface,
                maxLines = 3,
                overflow = TextOverflow.Ellipsis,
            )
            if (hint != null) {
                Text(hint, style = MaterialTheme.typography.bodySmall, color = palette.text2, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
        }
        trailing?.invoke()
    }
}

@Composable
private fun Field(
    value: String,
    onChange: (String) -> Unit,
    label: String,
    placeholder: String,
    keyboard: KeyboardType = KeyboardType.Text,
) {
    OutlinedTextField(
        value = value,
        onValueChange = onChange,
        singleLine = true,
        label = { Text(label) },
        placeholder = { if (placeholder.isNotEmpty()) Text(placeholder) },
        keyboardOptions = KeyboardOptions(keyboardType = keyboard, autoCorrectEnabled = false),
        modifier = Modifier.fillMaxWidth(),
    )
}

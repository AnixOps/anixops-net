package io.anixops.net.ui

import android.os.Build
import androidx.annotation.StringRes
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
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Check
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.anixops.net.R
import io.anixops.net.core.OnDemandSettings
import io.anixops.net.core.RuntimeSnapshot
import io.anixops.net.core.StatusFact
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette

/** What the settings page can ask for. Permissions are the app's job, so these come from [AnixApp]. */
data class SettingsActions(
    val setMode: (AppMode) -> Unit = {},
    /** Opens the system's per-app language page; null where Android has none (before 13). */
    val openLanguageSettings: (() -> Unit)? = null,
    val toggleNotification: (NotificationKind) -> Unit = {},
    /** Asks for the notification permission, or opens the app's notification settings once it cannot be asked. */
    val allowNotifications: () -> Unit = {},
    /** Asks for precise location, which the Wi-Fi name needs. */
    val allowPreciseLocation: () -> Unit = {},
    /** Opens the system's location settings, to turn location on. */
    val openLocationSettings: () -> Unit = {},
    /** Explains, then asks for location "all the time", for named rules in the background. */
    val allowBackgroundLocation: () -> Unit = {},
    val setOnDemandEnabled: (Boolean) -> Unit = {},
    val addWifiRule: (String) -> Unit = {},
    val setWifiRule: (ssid: String, action: String) -> Unit = { _, _ -> },
    val removeWifiRule: (String) -> Unit = {},
    val fillCurrentNetwork: () -> Unit = {},
    val setOtherWifi: (String) -> Unit = {},
    val setCellular: (String) -> Unit = {},
    val setOther: (String) -> Unit = {},
    val validateConfiguration: () -> Unit = {},
    val restartCore: () -> Unit = {},
    val setCrashReports: (Boolean) -> Unit = {},
    val createDiagnostics: () -> Unit = {},
    val openTerms: () -> Unit = {},
    val openPrivacy: () -> Unit = {},
    val openLogs: () -> Unit = {},
    val openTraffic: () -> Unit = {},
)

/** The Wi-Fi rule actions, in the order every client offers them. */
private val OnDemandActions = listOf("connect", "disconnect", "ignore")

/**
 * 设置: 使用方式 and language, notifications, the Wi-Fi rules, the proxy core, diagnostics and privacy, and about.
 * Android has no start-after-login or auto-connect (the VPN consent is asked in the window), as on the other
 * clients' Android column.
 */
@Composable
fun SettingsScreen(
    mode: AppMode,
    version: String,
    termsVersion: String,
    snapshot: RuntimeSnapshot?,
    state: SettingsState,
    actions: SettingsActions,
    modifier: Modifier = Modifier,
) {
    val enabled = !state.busy
    Page(stringResource(R.string.nav_settings), modifier) {
        GeneralSection(mode, actions)
        state.notifications?.let { notifications ->
            Column {
                Section(stringResource(R.string.notifications)) {
                    NotificationKind.entries.forEachIndexed { index, kind ->
                        if (index > 0) RowDivider()
                        SwitchRow(
                            stringResource(notificationLabel(kind)),
                            stringResource(notificationHint(kind)),
                            checked = when (kind) {
                                NotificationKind.Scripts -> notifications.scripts
                                NotificationKind.Connection -> notifications.connection
                                NotificationKind.Subscriptions -> notifications.subscriptions
                            },
                            enabled = enabled,
                            onChange = { actions.toggleNotification(kind) },
                        )
                    }
                }
                Note(stringResource(R.string.notifications_note))
                if (!state.system.notifications) {
                    SystemRow(
                        stringResource(R.string.notifications_blocked),
                        stringResource(R.string.action_allow),
                        actions.allowNotifications,
                    )
                }
            }
        }
        state.onDemand?.let { WifiRulesSection(it, state.currentSsid, state.system, enabled, actions) }
        Section(stringResource(R.string.activity)) {
            InfoRow(stringResource(R.string.logs), stringResource(R.string.action_view), onClick = actions.openLogs)
            InfoRow(
                stringResource(R.string.traffic_stats),
                stringResource(R.string.action_view),
                divider = true,
                onClick = actions.openTraffic,
            )
        }
        CoreSection(snapshot?.core, enabled, actions)
        DiagnosticsSection(state, enabled, actions)
        Section(stringResource(R.string.settings_about)) {
            InfoRow(stringResource(R.string.settings_version), version)
            InfoRow(
                stringResource(R.string.terms),
                stringResource(R.string.terms_version, termsVersion),
                divider = true,
                onClick = actions.openTerms,
            )
            InfoRow(
                stringResource(R.string.privacy_policy),
                stringResource(R.string.action_view),
                divider = true,
                onClick = actions.openPrivacy,
            )
        }
    }
}

@Composable
private fun GeneralSection(mode: AppMode, actions: SettingsActions) {
    Section(stringResource(R.string.settings_general)) {
        Column(Modifier.padding(BrandSpacing.md), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
            Text(stringResource(R.string.settings_usage), style = MaterialTheme.typography.bodyLarge)
            SingleChoiceSegmentedButtonRow(modifier = Modifier.fillMaxWidth()) {
                AppMode.entries.forEachIndexed { index, option ->
                    SegmentedButton(
                        selected = option == mode,
                        onClick = { actions.setMode(option) },
                        shape = SegmentedButtonDefaults.itemShape(index, AppMode.entries.size),
                    ) {
                        Text(stringResource(if (option == AppMode.Proxy) R.string.mode_proxy else R.string.mode_debug))
                    }
                }
            }
        }
        actions.openLanguageSettings?.let { open ->
            val locale = LocalConfiguration.current.locales[0]
            InfoRow(
                stringResource(R.string.settings_language),
                locale.getDisplayName(locale).replaceFirstChar { it.titlecase(locale) },
                divider = true,
                onClick = open,
            )
        }
    }
}

@StringRes
private fun notificationLabel(kind: NotificationKind): Int = when (kind) {
    NotificationKind.Scripts -> R.string.notifications_scripts
    NotificationKind.Connection -> R.string.notifications_connection
    NotificationKind.Subscriptions -> R.string.notifications_subscriptions
}

@StringRes
private fun notificationHint(kind: NotificationKind): Int = when (kind) {
    NotificationKind.Scripts -> R.string.notifications_scripts_hint
    NotificationKind.Connection -> R.string.notifications_connection_hint
    NotificationKind.Subscriptions -> R.string.notifications_subscriptions_hint
}

@Composable
private fun WifiRulesSection(
    settings: OnDemandSettings,
    currentSsid: String?,
    system: SystemAccess,
    enabled: Boolean,
    actions: SettingsActions,
) {
    var draft by rememberSaveable { mutableStateOf("") }
    LaunchedEffect(currentSsid) { if (currentSsid != null) draft = currentSsid }
    val name = draft.trim()
    val canAdd = SettingsViewModel.ssidFits(name) && settings.wifi.none { it.ssid == name }
    val add = {
        if (canAdd) {
            actions.addWifiRule(name)
            draft = ""
        }
    }
    Column {
        Section(stringResource(R.string.wifi_rules)) {
            SwitchRow(
                stringResource(R.string.wifi_rules_enabled),
                hint = null,
                checked = settings.enabled,
                enabled = enabled,
                onChange = actions.setOnDemandEnabled,
            )
            if (settings.enabled) {
                settings.wifi.forEach { rule ->
                    RowDivider()
                    ActionRow(rule.ssid, rule.action, enabled, { actions.setWifiRule(rule.ssid, it) }) {
                        IconButton(onClick = { actions.removeWifiRule(rule.ssid) }, enabled = enabled) {
                            Icon(Icons.Filled.Delete, contentDescription = stringResource(R.string.wifi_rule_remove, rule.ssid))
                        }
                    }
                }
                RowDivider()
                Column(
                    modifier = Modifier.padding(BrandSpacing.md),
                    verticalArrangement = Arrangement.spacedBy(BrandSpacing.xs),
                ) {
                    OutlinedTextField(
                        value = draft,
                        onValueChange = { draft = it },
                        singleLine = true,
                        label = { Text(stringResource(R.string.wifi_name)) },
                        keyboardOptions = KeyboardOptions(autoCorrectEnabled = false, imeAction = ImeAction.Done),
                        keyboardActions = KeyboardActions(onDone = { add() }),
                        modifier = Modifier.fillMaxWidth(),
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
                        OutlinedButton(onClick = actions.fillCurrentNetwork, enabled = enabled) {
                            Text(stringResource(R.string.wifi_current_network))
                        }
                        OutlinedButton(onClick = add, enabled = enabled && canAdd) {
                            Text(stringResource(R.string.wifi_add))
                        }
                    }
                }
                RowDivider()
                ActionRow(stringResource(R.string.wifi_other), settings.otherWifi, enabled, actions.setOtherWifi)
                RowDivider()
                ActionRow(stringResource(R.string.wifi_cellular), settings.cellular, enabled, actions.setCellular)
                RowDivider()
                ActionRow(stringResource(R.string.wifi_wired_other), settings.other, enabled, actions.setOther)
            }
        }
        Note(stringResource(R.string.wifi_rules_note))
        if (settings.enabled) {
            // Why named rules may not apply, and the way to fix it: precise location, location on, then
            // location "all the time" for the background.
            when {
                !system.preciseLocation -> SystemRow(
                    stringResource(R.string.wifi_precise_location_off),
                    stringResource(R.string.action_allow),
                    actions.allowPreciseLocation,
                )
                !system.locationOn -> SystemRow(
                    stringResource(R.string.wifi_location_off),
                    stringResource(R.string.action_open_settings),
                    actions.openLocationSettings,
                )
                Permissions.offersBackgroundLocation(
                    Build.VERSION.SDK_INT,
                    namedRules = settings.wifi.isNotEmpty(),
                    precise = system.preciseLocation,
                    background = system.backgroundLocation,
                ) -> SystemRow(
                    stringResource(R.string.wifi_background_location_off),
                    stringResource(R.string.action_allow),
                    actions.allowBackgroundLocation,
                )
            }
        }
    }
}

/** What the system does not allow yet, under a section, with the button that fixes it. */
@Composable
private fun SystemRow(text: String, action: String, onClick: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .padding(start = BrandSpacing.md),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs),
    ) {
        Text(
            text,
            style = MaterialTheme.typography.bodySmall,
            color = LocalBrandPalette.current.text2,
            modifier = Modifier.weight(1f),
        )
        TextButton(onClick = onClick) { Text(action) }
    }
}

/** A network and what the rules do on it, picked from a menu; [trailing] sits at the end (the delete button). */
@Composable
private fun ActionRow(
    label: String,
    action: String,
    enabled: Boolean,
    onChoose: (String) -> Unit,
    trailing: @Composable () -> Unit = {},
) {
    var open by remember { mutableStateOf(false) }
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .heightIn(min = RowMinHeight)
            .padding(start = BrandSpacing.md),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(
            label,
            style = MaterialTheme.typography.bodyLarge,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
        Box {
            TextButton(onClick = { open = true }, enabled = enabled) {
                Text(stringResource(actionLabel(action)))
            }
            DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
                OnDemandActions.forEach { option ->
                    DropdownMenuItem(
                        text = { Text(stringResource(actionLabel(option))) },
                        trailingIcon = { if (option == action) Icon(Icons.Filled.Check, contentDescription = null) },
                        onClick = {
                            open = false
                            if (option != action) onChoose(option)
                        },
                    )
                }
            }
        }
        trailing()
    }
}

@StringRes
private fun actionLabel(action: String): Int = when (action) {
    "connect" -> R.string.wifi_action_connect
    "disconnect" -> R.string.wifi_action_disconnect
    else -> R.string.wifi_action_ignore
}

@Composable
private fun CoreSection(core: StatusFact?, enabled: Boolean, actions: SettingsActions) {
    Column {
        Section(stringResource(R.string.core)) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .heightIn(min = RowMinHeight)
                    .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.sm),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs),
            ) {
                Text(stringResource(R.string.core), style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                if (core != null) {
                    Box(
                        Modifier
                            .size(StatusDot)
                            .clip(CircleShape)
                            .background(toneColor(core.tone)),
                    )
                }
                Text(
                    coreLabel(core?.label),
                    style = MaterialTheme.typography.bodyLarge,
                    color = LocalBrandPalette.current.text2,
                )
            }
            InfoRow(
                stringResource(R.string.core_validate),
                stringResource(R.string.core_validate_action),
                divider = true,
                onClick = if (enabled) actions.validateConfiguration else null,
            )
            InfoRow(
                stringResource(R.string.core_restart),
                stringResource(R.string.core_restart_action),
                divider = true,
                onClick = if (enabled) actions.restartCore else null,
            )
        }
        Note(stringResource(R.string.core_note))
    }
}

private val StatusDot = 8.dp

/** The client's short status words, translated; anything unknown is shown as is (`labels.ts` `factLabel`). */
@Composable
private fun coreLabel(label: String?): String = when {
    label == null -> stringResource(R.string.core_reading)
    label == "Running" || label.startsWith("Running (") || label.startsWith("Embedded engine running") ->
        stringResource(R.string.core_running)
    label == "Stopped" || label.startsWith("Stopped: ") -> stringResource(R.string.core_stopped)
    label == "Not used" -> stringResource(R.string.core_not_used)
    label.startsWith("Exited") -> stringResource(R.string.core_exited)
    else -> label
}

@Composable
private fun DiagnosticsSection(state: SettingsState, enabled: Boolean, actions: SettingsActions) {
    val health = state.health
    Section(stringResource(R.string.diagnostics)) {
        if (health != null) {
            SwitchRow(
                stringResource(R.string.crash_reports),
                stringResource(if (health.crashUploadConfigured) R.string.crash_reports_hint_upload else R.string.crash_reports_hint_local),
                checked = health.crashReports,
                enabled = enabled,
                onChange = actions.setCrashReports,
            )
            RowDivider()
        }
        InfoRow(
            stringResource(R.string.diagnostics_report),
            stringResource(R.string.diagnostics_report_action),
            onClick = if (enabled) actions.createDiagnostics else null,
        )
        health?.coldStartMs?.let { ms ->
            InfoRow(stringResource(R.string.cold_start), stringResource(R.string.duration_ms, ms), divider = true)
        }
    }
}

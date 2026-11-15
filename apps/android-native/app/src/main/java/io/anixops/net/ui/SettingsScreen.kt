package io.anixops.net.ui

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.SegmentedButton
import androidx.compose.material3.SegmentedButtonDefaults
import androidx.compose.material3.SingleChoiceSegmentedButtonRow
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import io.anixops.net.R
import io.anixops.net.ui.theme.BrandSpacing

/** 设置: what the preview has so far, the 使用方式 switch and the version. */
@Composable
fun SettingsScreen(
    mode: AppMode,
    version: String,
    onModeChange: (AppMode) -> Unit,
    modifier: Modifier = Modifier,
) {
    Page(stringResource(R.string.nav_settings), modifier) {
        Section(stringResource(R.string.settings_usage)) {
            SingleChoiceSegmentedButtonRow(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(BrandSpacing.md),
            ) {
                AppMode.entries.forEachIndexed { index, option ->
                    SegmentedButton(
                        selected = option == mode,
                        onClick = { onModeChange(option) },
                        shape = SegmentedButtonDefaults.itemShape(index, AppMode.entries.size),
                    ) {
                        Text(stringResource(if (option == AppMode.Proxy) R.string.mode_proxy else R.string.mode_debug))
                    }
                }
            }
        }
        Section(stringResource(R.string.settings_about)) {
            InfoRow(stringResource(R.string.settings_version), version)
        }
    }
}

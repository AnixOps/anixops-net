package io.anixops.net.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.Search
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedCard
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.unit.dp
import io.anixops.net.R
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette

/** Widest the choice grows on tablets. */
private val ModeChoiceMaxWidth = 560.dp

/** Test tag of each option, followed by the mode's stored key. */
const val MODE_OPTION_TAG = "mode-option-"

/**
 * Whether the first start asks how the app is going to be used, as on Tauri and Apple (`initialMode`). Someone with a
 * subscription or nodes came for the proxy and is not asked; null means the first overview has not answered yet.
 */
fun firstStartMode(stored: AppMode?, hasProfile: Boolean?): FirstStart = when {
    stored != null -> FirstStart.Chosen(stored)
    hasProfile == null -> FirstStart.Wait
    hasProfile -> FirstStart.Chosen(AppMode.Proxy)
    else -> FirstStart.Ask
}

sealed interface FirstStart {
    data class Chosen(val mode: AppMode) : FirstStart

    data object Wait : FirstStart

    data object Ask : FirstStart
}

/**
 * First start, after the user agreement: proxy or network debugging. Debug comes first as on the other clients
 * (`docs/debug-mode.md`); either can be changed later in Settings, so the screen says so and asks nothing else.
 */
@Composable
fun ModeChoiceScreen(onChoose: (AppMode) -> Unit, modifier: Modifier = Modifier) {
    val palette = LocalBrandPalette.current
    Surface(color = MaterialTheme.colorScheme.background, modifier = modifier.fillMaxSize()) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .windowInsetsPadding(WindowInsets.safeDrawing)
                .verticalScroll(rememberScrollState()),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Column(
                modifier = Modifier
                    .widthIn(max = ModeChoiceMaxWidth)
                    .fillMaxWidth()
                    .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.lg),
                verticalArrangement = Arrangement.spacedBy(BrandSpacing.md),
            ) {
                Text(stringResource(R.string.mode_choice_title), style = MaterialTheme.typography.headlineSmall)
                Text(stringResource(R.string.mode_choice_summary), style = MaterialTheme.typography.bodyMedium, color = palette.text2)
                ModeOption(AppMode.Proxy, Icons.Filled.Home, R.string.mode_proxy, R.string.mode_proxy_hint, onChoose)
                ModeOption(AppMode.Debug, Icons.Filled.Search, R.string.mode_debug, R.string.mode_debug_hint, onChoose)
            }
        }
    }
}

@Composable
private fun ModeOption(mode: AppMode, icon: ImageVector, title: Int, hint: Int, onChoose: (AppMode) -> Unit) {
    val palette = LocalBrandPalette.current
    OutlinedCard(
        onClick = { onChoose(mode) },
        modifier = Modifier
            .fillMaxWidth()
            .testTag(MODE_OPTION_TAG + mode.key),
    ) {
        Row(
            modifier = Modifier.padding(BrandSpacing.md),
            horizontalArrangement = Arrangement.spacedBy(BrandSpacing.md),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(icon, contentDescription = null, tint = palette.primary, modifier = Modifier.size(24.dp))
            Column(verticalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
                Text(stringResource(title), style = MaterialTheme.typography.titleMedium)
                Text(stringResource(hint), style = MaterialTheme.typography.bodyMedium, color = palette.text2)
            }
        }
    }
}

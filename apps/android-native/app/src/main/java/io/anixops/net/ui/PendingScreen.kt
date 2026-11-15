package io.anixops.net.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.res.stringResource
import io.anixops.net.R
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette

/** A page the preview app does not have yet (migration PRs 3–6 replace each one). */
@Composable
fun PendingScreen(title: String, modifier: Modifier = Modifier) {
    Page(title, modifier) {
        Section(title = null) {
            Column(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(BrandSpacing.md),
                verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs),
            ) {
                Text(stringResource(R.string.pending_title), style = MaterialTheme.typography.titleMedium)
                Text(
                    stringResource(R.string.pending_body),
                    style = MaterialTheme.typography.bodyMedium,
                    color = LocalBrandPalette.current.text2,
                )
            }
        }
    }
}

package io.anixops.net.ui

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette

/** Widest a page's content grows on tablets, so rows stay readable (docs/ui-ux.md layout rules). */
private val ContentMaxWidth = 640.dp

/** Minimum touch height of a row (docs/ui-ux.md: 48dp on Android). */
val RowMinHeight = 48.dp

/** A scrolling page: its title, then [content] in one column, clear of the status bar and centred on tablets. */
@Composable
fun Page(title: String, modifier: Modifier = Modifier, content: @Composable ColumnScope.() -> Unit) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Top + WindowInsetsSides.Horizontal)),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Column(
            modifier = Modifier
                .widthIn(max = ContentMaxWidth)
                .fillMaxWidth()
                .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.lg),
            verticalArrangement = Arrangement.spacedBy(BrandSpacing.lg),
        ) {
            Text(title, style = MaterialTheme.typography.headlineSmall)
            content()
        }
    }
}

/** A titled group of rows on one card, as the grouped lists of the other clients; [action] sits right of the title. */
@Composable
fun Section(
    title: String?,
    modifier: Modifier = Modifier,
    action: (@Composable () -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(modifier = modifier, verticalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
        if (title != null || action != null) {
            Row(
                modifier = Modifier
                    .fillMaxWidth()
                    .padding(start = BrandSpacing.md),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(
                    title.orEmpty(),
                    style = MaterialTheme.typography.labelMedium,
                    color = LocalBrandPalette.current.text2,
                    modifier = Modifier.weight(1f),
                )
                action?.invoke()
            }
        }
        Surface(
            shape = MaterialTheme.shapes.medium,
            color = LocalBrandPalette.current.surface,
            contentColor = LocalBrandPalette.current.text,
            modifier = Modifier.fillMaxWidth(),
        ) {
            Column(content = content)
        }
    }
}

/** One label and value line in a [Section]; [divider] draws the hairline above it, [onClick] makes it a button. */
@Composable
fun InfoRow(
    label: String,
    value: String,
    modifier: Modifier = Modifier,
    divider: Boolean = false,
    onClick: (() -> Unit)? = null,
) {
    if (divider) RowDivider()
    Row(
        modifier = modifier
            .fillMaxWidth()
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .heightIn(min = RowMinHeight)
            .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.sm),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(BrandSpacing.md),
    ) {
        Text(label, style = MaterialTheme.typography.bodyLarge)
        Text(
            value,
            style = MaterialTheme.typography.bodyLarge,
            color = if (onClick != null) LocalBrandPalette.current.primary else LocalBrandPalette.current.text2,
            textAlign = TextAlign.End,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
            modifier = Modifier.weight(1f),
        )
    }
}

/** The hairline between rows of a [Section], inset like the rows' text. */
@Composable
fun RowDivider() {
    HorizontalDivider(
        color = LocalBrandPalette.current.border,
        modifier = Modifier.padding(start = BrandSpacing.md),
    )
}

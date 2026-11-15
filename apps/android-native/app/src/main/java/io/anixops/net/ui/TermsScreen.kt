package io.anixops.net.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import io.anixops.net.R
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette

/** The list dot: its size, and its distance from the top of the first line, near the middle of the text. */
private val BulletSize = 4.dp
private val BulletTop = 8.dp

/** Widest the agreement's text grows on tablets. */
private val TermsMaxWidth = 720.dp

/**
 * The user agreement in full. With [onAccept] it is the first-start consent (as the other clients): accepting is
 * the one primary action, declining explains that the app cannot be used and leaves the screen up. Without it the
 * page is the 关于 › 用户协议 viewer and [onClose] leaves it.
 */
@Composable
fun TermsScreen(
    markdown: String,
    modifier: Modifier = Modifier,
    onAccept: (() -> Unit)? = null,
    onClose: () -> Unit = {},
) {
    val blocks = remember(markdown) { Terms.blocks(markdown) }
    var declined by rememberSaveable { mutableStateOf(false) }
    val palette = LocalBrandPalette.current
    val fullText = stringResource(R.string.terms_full_text)
    Surface(color = MaterialTheme.colorScheme.background, modifier = modifier.fillMaxSize()) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .windowInsetsPadding(WindowInsets.safeDrawing),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Column(
                modifier = Modifier
                    .widthIn(max = TermsMaxWidth)
                    .fillMaxWidth()
                    .weight(1f)
                    .padding(horizontal = BrandSpacing.md),
            ) {
                Column(
                    modifier = Modifier.padding(vertical = BrandSpacing.lg),
                    verticalArrangement = Arrangement.spacedBy(BrandSpacing.xs),
                ) {
                    Text(
                        stringResource(if (onAccept != null) R.string.terms_consent_title else R.string.terms),
                        style = MaterialTheme.typography.headlineSmall,
                    )
                    if (onAccept != null) {
                        Text(stringResource(R.string.terms_consent_summary), style = MaterialTheme.typography.bodyMedium, color = palette.text2)
                    }
                }
                Surface(
                    shape = MaterialTheme.shapes.medium,
                    color = palette.surface,
                    contentColor = palette.text,
                    modifier = Modifier
                        .fillMaxWidth()
                        .weight(1f),
                ) {
                    LazyColumn(
                        modifier = Modifier.semantics { contentDescription = fullText },
                        contentPadding = PaddingValues(BrandSpacing.md),
                        verticalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
                    ) {
                        items(blocks) { block -> TermsBlock(block) }
                    }
                }
                if (declined) {
                    Text(
                        stringResource(R.string.terms_declined),
                        style = MaterialTheme.typography.bodyMedium,
                        color = palette.danger,
                        modifier = Modifier.padding(top = BrandSpacing.sm),
                    )
                }
                Row(
                    modifier = Modifier
                        .fillMaxWidth()
                        .padding(vertical = BrandSpacing.md),
                    horizontalArrangement = Arrangement.spacedBy(BrandSpacing.xs, Alignment.End),
                ) {
                    if (onAccept != null) {
                        TextButton(onClick = { declined = true }) { Text(stringResource(R.string.terms_decline)) }
                        Button(onClick = onAccept) { Text(stringResource(R.string.terms_accept)) }
                    } else {
                        Button(onClick = onClose) { Text(stringResource(R.string.action_done)) }
                    }
                }
            }
        }
    }
}

@Composable
private fun TermsBlock(block: Terms.Block) {
    when (block.kind) {
        Terms.Kind.Title -> Text(block.text, style = MaterialTheme.typography.titleLarge)
        Terms.Kind.Heading -> Text(
            block.text,
            style = MaterialTheme.typography.titleMedium.copy(fontWeight = FontWeight.SemiBold),
            modifier = Modifier.padding(top = BrandSpacing.xs),
        )
        Terms.Kind.Paragraph -> Text(block.text, style = MaterialTheme.typography.bodyMedium)
        Terms.Kind.Item -> Row {
            // A drawn dot: the font's bullet sits high beside Chinese text.
            Box(
                Modifier
                    .padding(top = BulletTop)
                    .size(BulletSize)
                    .clip(CircleShape)
                    .background(LocalBrandPalette.current.text2),
            )
            Spacer(Modifier.width(BrandSpacing.xs))
            Text(block.text, style = MaterialTheme.typography.bodyMedium)
        }
    }
}

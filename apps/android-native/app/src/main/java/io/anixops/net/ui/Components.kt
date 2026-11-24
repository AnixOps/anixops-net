package io.anixops.net.ui

import androidx.annotation.StringRes
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.WindowInsetsSides
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.only
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Switch
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
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.res.pluralStringResource
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import io.anixops.net.R
import io.anixops.net.ui.theme.BrandSpacing
import io.anixops.net.ui.theme.LocalBrandPalette

/** Widest a page's content grows on tablets, so rows stay readable (docs/ui-ux.md layout rules). */
internal val ContentMaxWidth = 640.dp

/** Minimum touch height of a row (docs/ui-ux.md: 48dp on Android). */
val RowMinHeight = 48.dp

/**
 * Test tags the density checks count (DensityTest, docs/ui-ux.md N25-N27): a page's content column, each [Section] and
 * each row in one, and the list of things the user adds with its items. They change no layout.
 */
internal const val PAGE_CONTENT_TAG = "page-content"
internal const val SECTION_TAG = "section"
internal const val ROW_TAG = "row"
internal const val USER_LIST_TAG = "user-list"
internal const val USER_ITEM_TAG = "user-item"

/**
 * A scrolling page: its title, then [content] in one column, clear of the status bar and centred on tablets.
 * [onBack] puts a back button before the title, for a page opened from another.
 */
@Composable
fun Page(
    title: String,
    modifier: Modifier = Modifier,
    onBack: (() -> Unit)? = null,
    content: @Composable ColumnScope.() -> Unit,
) {
    Column(
        modifier = modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState())
            .windowInsetsPadding(WindowInsets.safeDrawing.only(WindowInsetsSides.Top + WindowInsetsSides.Horizontal)),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Column(
            modifier = Modifier
                .testTag(PAGE_CONTENT_TAG)
                .widthIn(max = ContentMaxWidth)
                .fillMaxWidth()
                .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.lg),
            verticalArrangement = Arrangement.spacedBy(BrandSpacing.lg),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                if (onBack != null) {
                    IconButton(onClick = onBack) {
                        Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = stringResource(R.string.action_back))
                    }
                }
                Text(title, style = MaterialTheme.typography.headlineSmall)
            }
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
    Column(modifier = modifier.testTag(SECTION_TAG), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xs)) {
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
            .testTag(ROW_TAG)
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

/** A switch with its name and sentence; the whole row toggles it. */
@Composable
fun SwitchRow(label: String, hint: String?, checked: Boolean, enabled: Boolean, onChange: (Boolean) -> Unit) {
    Row(
        modifier = Modifier
            .testTag(ROW_TAG)
            .fillMaxWidth()
            .toggleable(value = checked, enabled = enabled, role = Role.Switch, onValueChange = onChange)
            .heightIn(min = RowMinHeight)
            .padding(horizontal = BrandSpacing.md, vertical = BrandSpacing.xs),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(BrandSpacing.sm),
    ) {
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(BrandSpacing.xxs)) {
            Text(label, style = MaterialTheme.typography.bodyLarge)
            if (hint != null) Text(hint, style = MaterialTheme.typography.bodySmall, color = LocalBrandPalette.current.text2)
        }
        Switch(checked = checked, onCheckedChange = null, enabled = enabled)
    }
}

/** A list of things the user adds shows this many items, then folds behind "show all" (N26). */
internal const val FOLDED_ITEM_COUNT = 5

/**
 * The items of a list the user adds to, in one column tagged [USER_LIST_TAG] with each item tagged [USER_ITEM_TAG]:
 * the first [collapsed] and, when there are more, a "show all" / "show less" button under them (docs/ui-ux.md N26).
 * Hairlines go between the items, and before the first when [leadingDivider]; [showAll] names the button.
 */
@Composable
fun <T> FoldedList(
    items: List<T>,
    modifier: Modifier = Modifier,
    collapsed: Int = FOLDED_ITEM_COUNT,
    leadingDivider: Boolean = false,
    showAll: @Composable (Int) -> String = { pluralStringResource(R.plurals.list_show_all, it, it) },
    item: @Composable (T) -> Unit,
) {
    val foldable = items.size > collapsed
    var expanded by rememberSaveable { mutableStateOf(false) }
    val shown = if (foldable && !expanded) items.take(collapsed) else items
    Column(modifier.fillMaxWidth().testTag(USER_LIST_TAG)) {
        shown.forEachIndexed { index, entry ->
            if (index > 0 || leadingDivider) RowDivider()
            Box(Modifier.fillMaxWidth().testTag(USER_ITEM_TAG)) { item(entry) }
        }
    }
    if (foldable) {
        RowDivider()
        TextButton(onClick = { expanded = !expanded }, modifier = Modifier.fillMaxWidth()) {
            Text(if (expanded) stringResource(R.string.plugins_show_less) else showAll(items.size))
        }
    }
}

/** The sentence under a section, as the other clients' card descriptions. */
@Composable
fun Note(text: String) {
    Text(
        text,
        style = MaterialTheme.typography.bodySmall,
        color = LocalBrandPalette.current.text2,
        modifier = Modifier.padding(start = BrandSpacing.md, end = BrandSpacing.md, top = BrandSpacing.xs),
    )
}

/** The ⋮ button and its menu; [items] get a `close` to call before acting. */
@Composable
fun OverflowMenu(label: String, enabled: Boolean, items: @Composable (close: () -> Unit) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { open = true }, enabled = enabled) {
            Icon(Icons.Filled.MoreVert, contentDescription = label)
        }
        DropdownMenu(expanded = open, onDismissRequest = { open = false }) {
            items { open = false }
        }
    }
}

@Composable
fun MenuItem(label: Int, close: () -> Unit, onClick: () -> Unit) {
    DropdownMenuItem(
        text = { Text(stringResource(label)) },
        onClick = {
            close()
            onClick()
        },
    )
}

@Composable
fun TextDialog(
    title: String,
    label: String,
    initial: String,
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit,
    @StringRes confirm: Int = R.string.action_save,
) {
    var text by rememberSaveable { mutableStateOf(initial) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            OutlinedTextField(
                value = text,
                onValueChange = { text = it },
                singleLine = true,
                label = { Text(label) },
                modifier = Modifier.fillMaxWidth(),
            )
        },
        confirmButton = {
            TextButton(onClick = { onConfirm(text) }, enabled = text.isNotBlank()) { Text(stringResource(confirm)) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

@Composable
fun ConfirmDialog(title: String, onConfirm: () -> Unit, onDismiss: () -> Unit) {
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        confirmButton = {
            TextButton(onClick = onConfirm) {
                Text(stringResource(R.string.action_remove), color = LocalBrandPalette.current.danger)
            }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text(stringResource(R.string.action_cancel)) } },
    )
}

/** One point of a list; the dot is drawn because the font's bullet sits high beside Chinese text. */
@Composable
fun BulletLine(text: String) {
    Row {
        Box(
            Modifier
                .padding(top = BulletTop)
                .size(BulletSize)
                .clip(CircleShape)
                .background(LocalBrandPalette.current.text2),
        )
        Spacer(Modifier.width(BrandSpacing.xs))
        Text(text, style = MaterialTheme.typography.bodyMedium)
    }
}

/** Puts the dot on the first line's middle at bodyMedium (20sp line height). */
private val BulletTop = 8.dp
private val BulletSize = 4.dp

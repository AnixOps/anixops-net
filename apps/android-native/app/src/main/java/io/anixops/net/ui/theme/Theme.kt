package io.anixops.net.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.staticCompositionLocalOf

/** The palette in use, for what Material's colour roles do not name (success, hero gradient, soft fills). */
val LocalBrandPalette = staticCompositionLocalOf { BrandPalette.Light }

/**
 * Material 3 over the AnixOps Design tokens: fixed brand colours (no dynamic colour), light or dark as the system
 * is (docs/architecture/android-native-compose.md §6).
 */
@Composable
fun AnixTheme(dark: Boolean = isSystemInDarkTheme(), content: @Composable () -> Unit) {
    val palette = if (dark) BrandPalette.Dark else BrandPalette.Light
    CompositionLocalProvider(LocalBrandPalette provides palette) {
        MaterialTheme(
            colorScheme = colorScheme(palette, dark),
            typography = typography,
            shapes = shapes,
            content = content,
        )
    }
}

private fun colorScheme(palette: BrandPalette, dark: Boolean): ColorScheme {
    val base = if (dark) darkColorScheme() else lightColorScheme()
    return base.copy(
        primary = palette.primaryFill,
        onPrimary = palette.onPrimary,
        primaryContainer = palette.soft,
        onPrimaryContainer = palette.primary,
        secondary = palette.primary,
        onSecondary = palette.onPrimary,
        secondaryContainer = palette.soft,
        onSecondaryContainer = palette.primary,
        background = palette.bg,
        onBackground = palette.text,
        surface = palette.bg,
        onSurface = palette.text,
        surfaceVariant = palette.surface,
        onSurfaceVariant = palette.text2,
        surfaceContainerLowest = palette.surface,
        surfaceContainerLow = palette.surface,
        surfaceContainer = palette.surface,
        surfaceContainerHigh = palette.surface,
        surfaceContainerHighest = palette.surface,
        outline = palette.borderStrong,
        outlineVariant = palette.border,
        error = palette.dangerFill,
        onError = palette.onPrimary,
        errorContainer = palette.dangerSoft,
        onErrorContainer = palette.danger,
    )
}

private val typography = Typography(
    displaySmall = BrandType.display,
    headlineSmall = BrandType.display,
    titleLarge = BrandType.title,
    titleMedium = BrandType.headline,
    titleSmall = BrandType.callout,
    bodyLarge = BrandType.body,
    bodyMedium = BrandType.callout,
    bodySmall = BrandType.caption,
    labelLarge = BrandType.callout,
    labelMedium = BrandType.caption,
    labelSmall = BrandType.micro,
)

private val shapes = Shapes(
    extraSmall = RoundedCornerShape(BrandRadius.badge),
    small = RoundedCornerShape(BrandRadius.control),
    medium = RoundedCornerShape(BrandRadius.card),
    large = RoundedCornerShape(BrandRadius.hero),
)

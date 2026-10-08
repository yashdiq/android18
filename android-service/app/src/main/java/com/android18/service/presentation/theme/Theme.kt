package com.android18.service.presentation.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable

private val LightColors = lightColorScheme(
    primary = Slate900,
    onPrimary = Slate50,
    background = Slate50,
    onBackground = Slate900,
    surface = androidx.compose.ui.graphics.Color.White,
    onSurface = Slate900,
    surfaceVariant = Slate100,
    onSurfaceVariant = Slate500,
    outline = Slate200,
    outlineVariant = Slate200,
    secondary = Slate700,
    onSecondary = Slate50,
    error = Rose,
    onError = Slate50,
)

private val DarkColors = darkColorScheme(
    primary = Slate100OnDark,
    onPrimary = Slate900,
    background = Slate950,
    onBackground = Slate100OnDark,
    surface = DarkSurface,
    onSurface = Slate100OnDark,
    surfaceVariant = DarkSurfaceVariant,
    onSurfaceVariant = Slate400,
    outline = DarkOutline,
    outlineVariant = DarkOutline,
    secondary = Slate300,
    onSecondary = Slate900,
    error = Rose,
    onError = Slate900,
)

@Composable
fun Android18Theme(
    darkTheme: Boolean = isSystemInDarkTheme(),
    content: @Composable () -> Unit,
) {
    MaterialTheme(
        colorScheme = if (darkTheme) DarkColors else LightColors,
        typography = AppTypography,
        content = content,
    )
}

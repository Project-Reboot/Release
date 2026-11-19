package com.rebootteam.launcher.ui

import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily

object Palette {
    val Window = Color(0xFF000000)
    val Main = Color(0xFF0E0830)
    val NavyBottom = Color(0xFF19144D)
    val Header = Color(0xFF1B1B1B)
    val Side = Color(0xFF0E0E0E)
    val Line = Color(0xFF2B2B2B)
    val TitleGrey = Color(0xFF8C8C8C)
    val TabGrey = Color(0xFF7A7A7A)
    val Green = Color(0xFF1DBA44)
    val GreenLight = Color(0xFF46DC69)
    val Accent = Color(0xFFFF78F0)
    val Magenta = Color(0xFFFF15E9)
    val Text = Color(0xFFF5F5FA)
    val Body = Color(0xFFE4E4EC)
    val Muted = Color(0xFFCDCDD7)
    val Faint = Color(0xFF9191A0)
    val Warn = Color(0xFFFFCD60)
    val Error = Color(0xFFFF7870)
    val Track = Color(0xFF121212)
    val Thumb = Color(0xFF141414)
}

@Composable
fun buttonFont(): FontFamily? {
    val context = LocalContext.current
    return remember(context) {
        val id = context.resources.getIdentifier("blocktopia", "font", context.packageName)
        if (id != 0) FontFamily(Font(id)) else null
    }
}

@Composable
fun LauncherTheme(content: @Composable () -> Unit) {
    MaterialTheme(
        colorScheme = darkColorScheme(
            primary = Palette.Green,
            onPrimary = Color.White,
            secondary = Palette.Accent,
            background = Palette.Main,
            onBackground = Palette.Text,
            surface = Palette.Main,
            onSurface = Palette.Text,
            error = Palette.Error,
        ),
        content = content,
    )
}

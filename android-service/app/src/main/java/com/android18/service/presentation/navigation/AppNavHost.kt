package com.android18.service.presentation.navigation

import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Scaffold
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import com.android18.service.presentation.feature.scanner.ScannerScreen
import com.android18.service.presentation.feature.server.ServerScreen
import com.android18.service.presentation.feature.settings.SettingsScreen

object Routes {
    const val SERVER = "server"
    const val SETTINGS = "settings"
    const val SCANNER = "scanner"
}

/**
 * App shell: the server dashboard is the whole app; Settings is pushed on
 * top from the toolbar gear (no tab bar — the phone is a transfer endpoint,
 * browsing/AI/shell live in the desktop client).
 */
@Composable
fun AppRoot() {
    val navController = rememberNavController()
    Scaffold { padding ->
        NavHost(
            navController = navController,
            startDestination = Routes.SERVER,
            modifier = Modifier.padding(padding),
            enterTransition = { slideInHorizontally(tween(220)) { it / 8 } + fadeIn(tween(220)) },
            exitTransition = { fadeOut(tween(150)) },
            popEnterTransition = { fadeIn(tween(220)) },
            popExitTransition = {
                slideOutHorizontally(tween(220)) { it / 8 } + fadeOut(tween(150))
            },
        ) {
            composable(Routes.SERVER) {
                ServerScreen(
                    onOpenSettings = { navController.navigate(Routes.SETTINGS) },
                    onOpenScanner = { navController.navigate(Routes.SCANNER) },
                )
            }
            composable(Routes.SETTINGS) {
                SettingsScreen(onBack = { navController.popBackStack() })
            }
            composable(Routes.SCANNER) {
                ScannerScreen(onDone = { navController.popBackStack() })
            }
        }
    }
}

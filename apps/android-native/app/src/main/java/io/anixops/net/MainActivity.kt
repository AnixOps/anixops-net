package io.anixops.net

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.lifecycle.viewmodel.compose.viewModel
import io.anixops.net.ui.ActivityViewModel
import io.anixops.net.ui.AnixApp
import io.anixops.net.ui.AppViewModel
import io.anixops.net.ui.ExtensionsViewModel
import io.anixops.net.ui.ProxiesViewModel
import io.anixops.net.ui.RulesViewModel
import io.anixops.net.ui.SettingsViewModel
import io.anixops.net.ui.theme.AnixTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val app = application as AnixApplication
        val preferences = getSharedPreferences(AppViewModel.PREFERENCES, MODE_PRIVATE)
        setContent {
            AnixTheme {
                AnixApp(
                    app = viewModel { AppViewModel(app.core, preferences) },
                    proxies = viewModel { ProxiesViewModel(app.core) },
                    rules = viewModel { RulesViewModel(app.core) },
                    settings = viewModel { SettingsViewModel(app.core) },
                    activity = viewModel { ActivityViewModel(app.core) },
                    extensions = viewModel { ExtensionsViewModel(app.core, preferences) },
                    version = BuildConfig.VERSION_NAME,
                )
            }
        }
    }
}

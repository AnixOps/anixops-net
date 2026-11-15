package io.anixops.net

import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.lifecycle.viewmodel.compose.viewModel
import io.anixops.net.ui.AnixApp
import io.anixops.net.ui.AppViewModel
import io.anixops.net.ui.theme.AnixTheme

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        val app = application as AnixApplication
        val preferences = getSharedPreferences(AppViewModel.PREFERENCES, MODE_PRIVATE)
        setContent {
            AnixTheme {
                AnixApp(viewModel { AppViewModel(app.core, preferences) }, BuildConfig.VERSION_NAME)
            }
        }
    }
}

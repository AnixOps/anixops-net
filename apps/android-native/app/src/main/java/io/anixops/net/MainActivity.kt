package io.anixops.net

import android.content.Intent
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
import io.anixops.net.ui.MeshViewModel
import io.anixops.net.ui.RequestsViewModel
import io.anixops.net.ui.RulesViewModel
import io.anixops.net.ui.SettingsViewModel
import io.anixops.net.ui.theme.AnixTheme
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.flow.receiveAsFlow

class MainActivity : ComponentActivity() {
    /** `anixops://` links this activity was opened with, for the app to follow. */
    private val links = Channel<String>(Channel.BUFFERED)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        // A link that opened the activity counts once, not again when it is recreated (a rotation).
        if (savedInstanceState == null) receive(intent)
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
                    requests = viewModel { RequestsViewModel(app.core) },
                    mesh = viewModel { MeshViewModel(app.core) },
                    version = BuildConfig.VERSION_NAME,
                    links = links.receiveAsFlow(),
                    preferences = preferences,
                )
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        receive(intent)
    }

    private fun receive(intent: Intent?) {
        val link = intent?.takeIf { it.action == Intent.ACTION_VIEW }?.dataString ?: return
        if (link.startsWith("$LINK_SCHEME:", ignoreCase = true)) links.trySend(link)
    }

    private companion object {
        const val LINK_SCHEME = "anixops"
    }
}

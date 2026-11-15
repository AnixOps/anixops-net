package io.anixops.net.ui

import android.app.Activity
import android.net.VpnService
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.annotation.StringRes
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ExitToApp
import androidx.compose.material.icons.automirrored.filled.List
import androidx.compose.material.icons.filled.Build
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.Home
import androidx.compose.material.icons.filled.LocationOn
import androidx.compose.material.icons.filled.Search
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteScaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import io.anixops.net.R

/**
 * The pages of each 使用方式, in the order every client shows them (docs/architecture/android-native-compose.md §5):
 * proxy 首页·代理·规则·扩展·设置, debug 请求·改写·出口·设置.
 */
enum class Destination(@param:StringRes val label: Int, val icon: ImageVector, val mode: AppMode?) {
    Home(R.string.nav_home, Icons.Filled.Home, AppMode.Proxy),
    Proxies(R.string.nav_proxies, Icons.Filled.LocationOn, AppMode.Proxy),
    Rules(R.string.nav_rules, Icons.AutoMirrored.Filled.List, AppMode.Proxy),
    Extensions(R.string.nav_extensions, Icons.Filled.Build, AppMode.Proxy),
    Requests(R.string.nav_requests, Icons.Filled.Search, AppMode.Debug),
    Rewrite(R.string.nav_rewrite, Icons.Filled.Edit, AppMode.Debug),
    Egress(R.string.nav_egress, Icons.AutoMirrored.Filled.ExitToApp, AppMode.Debug),
    Settings(R.string.nav_settings, Icons.Filled.Settings, null),
    ;

    companion object {
        fun of(mode: AppMode): List<Destination> = entries.filter { it.mode == null || it.mode == mode }
    }
}

/**
 * The app over [viewModel]: reads the client's snapshot while the screen is started and asks for the VPN
 * consent before the first connect.
 */
@Composable
fun AnixApp(viewModel: AppViewModel, version: String) {
    val state by viewModel.state.collectAsStateWithLifecycle()
    val mode by viewModel.mode.collectAsStateWithLifecycle()
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(lifecycle) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { viewModel.poll() }
    }
    val context = LocalContext.current
    val denied = stringResource(R.string.home_vpn_denied)
    val consent = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        if (result.resultCode == Activity.RESULT_OK) viewModel.connect() else viewModel.showNotice(denied)
    }
    AnixScaffold(
        state = state,
        mode = mode,
        version = version,
        onConnect = {
            val ask = VpnService.prepare(context)
            if (ask == null) viewModel.connect() else consent.launch(ask)
        },
        onDisconnect = viewModel::disconnect,
        onModeChange = viewModel::setMode,
    )
}

/** The navigation and the current page: a bottom bar on phones, a side rail on wide windows. */
@Composable
fun AnixScaffold(
    state: UiState,
    mode: AppMode,
    version: String,
    onConnect: () -> Unit,
    onDisconnect: () -> Unit,
    onModeChange: (AppMode) -> Unit,
    modifier: Modifier = Modifier,
    initial: Destination? = null,
) {
    val destinations = Destination.of(mode)
    var selected by rememberSaveable(mode) { mutableStateOf(initial?.takeIf { it in destinations } ?: destinations.first()) }
    NavigationSuiteScaffold(
        modifier = modifier,
        containerColor = MaterialTheme.colorScheme.background,
        navigationSuiteItems = {
            destinations.forEach { destination ->
                item(
                    selected = destination == selected,
                    onClick = { selected = destination },
                    icon = { Icon(destination.icon, contentDescription = null) },
                    label = { Text(stringResource(destination.label)) },
                )
            }
        },
    ) {
        when (selected) {
            Destination.Home -> HomeScreen(state, onConnect, onDisconnect)
            Destination.Settings -> SettingsScreen(mode, version, onModeChange)
            else -> PendingScreen(stringResource(selected.label))
        }
    }
}

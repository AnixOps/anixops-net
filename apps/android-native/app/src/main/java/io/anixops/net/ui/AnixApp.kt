package io.anixops.net.ui

import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.net.VpnService
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Box
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
import androidx.compose.material3.SnackbarHost
import androidx.compose.material3.SnackbarHostState
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.navigationsuite.NavigationSuiteScaffold
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
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
 * The app over its view models: reads the client while the screen is started, asks for the VPN consent before
 * the first connect, and shows what commands answered in a snackbar.
 */
@Composable
fun AnixApp(app: AppViewModel, proxies: ProxiesViewModel, version: String) {
    val state by app.state.collectAsStateWithLifecycle()
    val mode by app.mode.collectAsStateWithLifecycle()
    val proxiesState by proxies.state.collectAsStateWithLifecycle()
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(lifecycle) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { app.poll() }
    }
    val context = LocalContext.current
    val snackbar = remember { SnackbarHostState() }
    LaunchedEffect(app) { app.messages.collect { snackbar.showSnackbar(it) } }
    LaunchedEffect(proxies) {
        proxies.events.collect { event ->
            when (event) {
                is ProxiesEvent.Message -> if (event.text.isNotBlank()) snackbar.showSnackbar(event.text)
                is ProxiesEvent.ShareLink -> {
                    val clipboard = context.getSystemService(ClipboardManager::class.java)
                    clipboard.setPrimaryClip(ClipData.newPlainText(event.node, event.link))
                    snackbar.showSnackbar(context.getString(R.string.proxies_shared, event.node))
                }
            }
        }
    }
    val denied = stringResource(R.string.home_vpn_denied)
    val consent = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        if (result.resultCode == Activity.RESULT_OK) app.connect() else app.showNotice(denied)
    }
    AnixScaffold(
        state = state,
        mode = mode,
        version = version,
        home = HomeActions(
            connect = {
                val ask = VpnService.prepare(context)
                if (ask == null) app.connect() else consent.launch(ask)
            },
            disconnect = app::disconnect,
            setRoutingMode = app::setRoutingMode,
            closeConnection = { app.closeConnection(it) },
            closeAll = { app.closeAllConnections() },
        ),
        proxiesState = proxiesState,
        proxies = ProxiesActions(
            search = proxies::search,
            select = proxies::select,
            testAll = proxies::testShown,
            selectFastest = proxies::selectFastest,
            share = { id, label -> proxies.share(id, label) },
            loadForm = proxies::form,
            edit = proxies::edit,
            remove = proxies::remove,
            importSubscription = proxies::importSubscription,
            useSubscription = proxies::useSubscription,
            updateSubscription = proxies::updateSubscription,
            renameSubscription = proxies::renameSubscription,
            setInterval = proxies::setInterval,
            removeSubscription = proxies::removeSubscription,
        ),
        onModeChange = app::setMode,
        snackbar = snackbar,
        pollProxies = {
            lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { proxies.poll() }
        },
    )
}

/** The navigation and the current page: a bottom bar on phones, a side rail on wide windows. */
@Composable
fun AnixScaffold(
    state: UiState,
    mode: AppMode,
    version: String,
    onModeChange: (AppMode) -> Unit,
    modifier: Modifier = Modifier,
    home: HomeActions = HomeActions(),
    proxiesState: ProxiesState = ProxiesState(),
    proxies: ProxiesActions = ProxiesActions(),
    snackbar: SnackbarHostState = remember { SnackbarHostState() },
    pollProxies: suspend () -> Unit = {},
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
        Box {
            when (selected) {
                Destination.Home -> HomeScreen(state, home)
                Destination.Proxies -> {
                    LaunchedEffect(Unit) { pollProxies() }
                    ProxiesScreen(proxiesState, proxies)
                }
                Destination.Settings -> SettingsScreen(mode, version, onModeChange)
                else -> PendingScreen(stringResource(selected.label))
            }
            SnackbarHost(snackbar, modifier = Modifier.align(Alignment.BottomCenter))
        }
    }
}

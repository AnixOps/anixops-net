package io.anixops.net.ui

import android.Manifest
import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.net.VpnService
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.BackHandler
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
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.core.content.ContextCompat
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import io.anixops.net.R
import java.util.Locale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

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
fun AnixApp(
    app: AppViewModel,
    proxies: ProxiesViewModel,
    rules: RulesViewModel,
    settings: SettingsViewModel,
    version: String,
) {
    val state by app.state.collectAsStateWithLifecycle()
    val mode by app.mode.collectAsStateWithLifecycle()
    val proxiesState by proxies.state.collectAsStateWithLifecycle()
    val rulesState by rules.state.collectAsStateWithLifecycle()
    val settingsState by settings.state.collectAsStateWithLifecycle()
    val termsAccepted by app.termsAccepted.collectAsStateWithLifecycle()
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
    LaunchedEffect(rules) {
        rules.events.collect { event ->
            val text = when (event) {
                is RulesEvent.Message -> event.text
                is RulesEvent.RuleSetsStarted ->
                    context.resources.getQuantityString(R.plurals.rules_sets_started, event.count, event.count)
            }
            if (text.isNotBlank()) snackbar.showSnackbar(text)
        }
    }
    LaunchedEffect(settings) {
        settings.events.collect { event ->
            val text = when (event) {
                is SettingsEvent.Message -> event.text
                is SettingsEvent.Resource -> context.getString(event.id)
            }
            if (text.isNotBlank()) snackbar.showSnackbar(text)
        }
    }
    val terms by produceState<String?>(null) {
        value = withContext(Dispatchers.IO) { context.assets.open(Terms.ASSET).bufferedReader().use { it.readText() } }
    }
    val termsText = terms ?: return
    val termsVersion = remember(termsText) { Terms.version(termsText) }
    if (Terms.needsConsent(termsAccepted, termsVersion)) {
        TermsScreen(termsText, onAccept = { app.acceptTerms(termsVersion) })
        return
    }
    var readingTerms by rememberSaveable { mutableStateOf(false) }
    if (readingTerms) {
        BackHandler { readingTerms = false }
        TermsScreen(termsText, onClose = { readingTerms = false })
        return
    }
    val language = clientLanguage(LocalConfiguration.current.locales[0])
    val settingsActions = rememberSettingsActions(settings, app) { readingTerms = true }
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
        rulesState = rulesState,
        rules = RulesActions(
            setRoutingMode = app::setRoutingMode,
            refreshRoutingRules = rules::refreshRoutingRules,
            saveMyRules = rules::saveMyRules,
            selectGroupOutbound = rules::selectGroupOutbound,
            updateRuleSets = rules::updateRuleSets,
            setDnsPreset = app::setDnsPreset,
            saveDns = rules::saveDns,
            setBlockQuic = app::setBlockQuic,
        ),
        settingsState = settingsState,
        settings = settingsActions,
        termsVersion = termsVersion,
        onModeChange = app::setMode,
        snackbar = snackbar,
        pollProxies = {
            lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { proxies.poll() }
        },
        pollRules = {
            lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { rules.poll() }
        },
        pollSettings = {
            lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { settings.poll(language) }
        },
    )
}

/**
 * The settings actions with the permissions they need: the location permission for the Wi-Fi name (asked when the
 * rules are turned on, and before reading the current network), and on Android 13+ the notification permission
 * when a notification switch is touched.
 */
@Composable
private fun rememberSettingsActions(settings: SettingsViewModel, app: AppViewModel, openTerms: () -> Unit): SettingsActions {
    val context = LocalContext.current
    var afterLocation by remember { mutableStateOf<(() -> Unit)?>(null) }
    val location = rememberLauncherForActivityResult(ActivityResultContracts.RequestMultiplePermissions()) {
        afterLocation?.invoke()
        afterLocation = null
    }
    val notifications = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) {}
    fun granted(permission: String) =
        ContextCompat.checkSelfPermission(context, permission) == PackageManager.PERMISSION_GRANTED
    fun withLocation(then: (() -> Unit)?) {
        if (granted(Manifest.permission.ACCESS_FINE_LOCATION)) {
            then?.invoke()
        } else {
            afterLocation = then
            location.launch(arrayOf(Manifest.permission.ACCESS_FINE_LOCATION, Manifest.permission.ACCESS_COARSE_LOCATION))
        }
    }
    val languageSettings = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        {
            try {
                context.startActivity(
                    Intent(Settings.ACTION_APP_LOCALE_SETTINGS, Uri.fromParts("package", context.packageName, null)),
                )
            } catch (error: ActivityNotFoundException) {
                // Some Android 13 builds leave the page out; the language then follows the system's.
            }
        }
    } else {
        null
    }
    return SettingsActions(
        setMode = app::setMode,
        openLanguageSettings = languageSettings,
        toggleNotification = { kind ->
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && !granted(Manifest.permission.POST_NOTIFICATIONS)) {
                notifications.launch(Manifest.permission.POST_NOTIFICATIONS)
            }
            settings.toggleNotification(kind)
        },
        setOnDemandEnabled = { enabled ->
            settings.setOnDemandEnabled(enabled)
            // The rules for cellular and other networks work without it; only the Wi-Fi name needs it.
            if (enabled) withLocation(null)
        },
        addWifiRule = settings::addWifiRule,
        setWifiRule = settings::setWifiRule,
        removeWifiRule = settings::removeWifiRule,
        fillCurrentNetwork = { withLocation(settings::fillCurrentNetwork) },
        setOtherWifi = settings::setOtherWifi,
        setCellular = settings::setCellular,
        setOther = settings::setOther,
        validateConfiguration = settings::validateConfiguration,
        restartCore = settings::restartCore,
        setCrashReports = settings::setCrashReports,
        createDiagnostics = settings::createDiagnostics,
        openTerms = openTerms,
    )
}

/** The client's code for the language of notification texts (`i18n.ts` `Language`). */
private fun clientLanguage(locale: Locale): String = when {
    locale.language != "zh" -> "en"
    locale.script == "Hant" || locale.country in setOf("TW", "HK", "MO") -> "zh-Hant"
    else -> "zh"
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
    rulesState: RulesState = RulesState(),
    rules: RulesActions = RulesActions(),
    settingsState: SettingsState = SettingsState(),
    settings: SettingsActions = SettingsActions(),
    termsVersion: String = "",
    snackbar: SnackbarHostState = remember { SnackbarHostState() },
    pollProxies: suspend () -> Unit = {},
    pollRules: suspend () -> Unit = {},
    pollSettings: suspend () -> Unit = {},
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
                Destination.Rules -> {
                    LaunchedEffect(Unit) { pollRules() }
                    RulesScreen(state.snapshot, state.busy, rulesState, rules)
                }
                Destination.Settings -> {
                    LaunchedEffect(Unit) { pollSettings() }
                    SettingsScreen(mode, version, termsVersion, state.snapshot, settingsState, settings.copy(setMode = onModeChange))
                }
                else -> PendingScreen(stringResource(selected.label))
            }
            SnackbarHost(snackbar, modifier = Modifier.align(Alignment.BottomCenter))
        }
    }
}

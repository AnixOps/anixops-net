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
import android.provider.OpenableColumns
import android.provider.Settings
import androidx.activity.compose.BackHandler
import androidx.activity.compose.LocalOnBackPressedDispatcherOwner
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.annotation.StringRes
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
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
import java.io.IOException
import java.util.Locale
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.emptyFlow
import kotlinx.coroutines.launch
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
    activity: ActivityViewModel,
    extensions: ExtensionsViewModel,
    requests: RequestsViewModel,
    version: String,
    /** `anixops://` links the activity received, followed once the user agreement is accepted. */
    links: Flow<String> = emptyFlow(),
) {
    val state by app.state.collectAsStateWithLifecycle()
    val mode by app.mode.collectAsStateWithLifecycle()
    val debugExit by app.debugExit.collectAsStateWithLifecycle()
    val proxiesState by proxies.state.collectAsStateWithLifecycle()
    val rulesState by rules.state.collectAsStateWithLifecycle()
    val settingsState by settings.state.collectAsStateWithLifecycle()
    val termsAccepted by app.termsAccepted.collectAsStateWithLifecycle()
    val logsState by activity.logs.collectAsStateWithLifecycle()
    val trafficState by activity.traffic.collectAsStateWithLifecycle()
    val extensionsState by extensions.state.collectAsStateWithLifecycle()
    val requestsState by requests.state.collectAsStateWithLifecycle()
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(lifecycle) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { app.poll() }
    }
    val context = LocalContext.current
    val locale by rememberUpdatedState(LocalConfiguration.current.locales[0])
    val snackbar = remember { SnackbarHostState() }
    LaunchedEffect(app) { app.messages.collect { snackbar.showSnackbar(it) } }
    val connected = state.overview?.state == "connected"
    val decrypting = state.snapshot?.httpsDecryptionEnabled == true
    LaunchedEffect(requests, connected, decrypting) { requests.setSetupInputs(connected, decrypting) }
    LaunchedEffect(requests) {
        requests.events.collect { event ->
            val text = when (event) {
                is RequestsEvent.Message -> event.text
                is RequestsEvent.Saved -> context.getString(R.string.request_saved, event.location)
                is RequestsEvent.Session -> context.getString(
                    when (event.change) {
                        SessionChange.Saved -> R.string.sessions_saved_message
                        SessionChange.Loaded -> R.string.sessions_loaded_message
                        SessionChange.Deleted -> R.string.sessions_deleted_message
                    },
                    sessionTitle(event.session, locale),
                )
            }
            if (text.isNotBlank()) snackbar.showSnackbar(text)
        }
    }
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
                RulesEvent.HitsCleared -> context.getString(R.string.rules_hits_cleared)
                RulesEvent.Tested -> ""
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
    LaunchedEffect(extensions) {
        extensions.messages.collect { if (it.isNotBlank()) snackbar.showSnackbar(it) }
    }
    LaunchedEffect(activity) {
        activity.events.collect { event ->
            val text = when (event) {
                is ActivityEvent.Message -> event.text
                ActivityEvent.LogsCleared -> context.getString(R.string.logs_cleared)
                ActivityEvent.StatsCleared -> context.getString(R.string.traffic_cleared)
                is ActivityEvent.CopyLogs -> {
                    val clipboard = context.getSystemService(ClipboardManager::class.java)
                    clipboard.setPrimaryClip(ClipData.newPlainText(context.getString(R.string.logs), event.text))
                    context.resources.getQuantityString(R.plurals.logs_copied, event.lines, event.lines)
                }
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
    // After the agreement, a new user picks proxy or debugging once; someone who already has a subscription or nodes
    // (an upgrade, or a subscription link) stays in proxy mode without being asked. A failed first read asks too.
    val storedMode by app.storedMode.collectAsStateWithLifecycle()
    val hasProfile = state.snapshot?.let { !it.subscription.isNullOrBlank() || state.nodeCount > 0 }
        ?: if (state.notice != null) false else null
    when (val first = firstStartMode(storedMode, hasProfile)) {
        FirstStart.Wait -> return
        FirstStart.Ask -> {
            ModeChoiceScreen(onChoose = app::setMode)
            return
        }
        is FirstStart.Chosen -> if (storedMode == null) LaunchedEffect(first.mode) { app.setMode(first.mode) }
    }
    var readingTerms by rememberSaveable { mutableStateOf(false) }
    if (readingTerms) {
        BackTo { readingTerms = false }
        TermsScreen(termsText, onClose = { readingTerms = false })
        return
    }
    val language = clientLanguage(LocalConfiguration.current.locales[0])
    val settingsActions = rememberSettingsActions(settings, app) { readingTerms = true }
    val extensionsActions = rememberExtensionsActions(extensions, snackbar)
    val denied = stringResource(R.string.home_vpn_denied)
    val consent = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        if (result.resultCode == Activity.RESULT_OK) app.connect() else app.showNotice(denied)
    }
    val connect = {
        val ask = VpnService.prepare(context)
        if (ask == null) app.connect() else consent.launch(ask)
    }
    val following by rememberUpdatedState(connect)
    LaunchedEffect(links) {
        links.collect { link -> app.followLink(link, connect = { following() }, offerImport = proxies::offerImport) }
    }
    LaunchedEffect(app) {
        app.links.collect { event ->
            val text = when (event) {
                is LinkEvent.Refused -> context.getString(
                    R.string.link_refused,
                    context.getString(linkReason(event.reason)) + (event.detail?.let { " ($it)" } ?: ""),
                )
                is LinkEvent.NoNode -> context.getString(R.string.link_no_node, event.node)
                is LinkEvent.NoMember -> context.getString(R.string.link_no_member, event.policy, event.member)
                LinkEvent.ImportOffered -> context.getString(R.string.link_import_offered)
            }
            snackbar.showSnackbar(text)
        }
    }
    AnixScaffold(
        state = state,
        mode = mode,
        version = version,
        home = HomeActions(
            connect = connect,
            disconnect = app::disconnect,
            setRoutingMode = app::setRoutingMode,
            closeConnection = { app.closeConnection(it) },
            closeAll = { app.closeAllConnections() },
            dismissNotice = app::clearNotice,
            // An empty offer opens the import form on 代理 (or 出口), like an `anixops://import` link.
            openImport = { proxies.offerImport("") },
        ),
        debugExit = debugExit,
        egress = EgressActions(setExit = app::setDebugExit),
        requestsState = requestsState,
        requests = RequestsActions(
            setCapture = requests::setCapture,
            clear = requests::clear,
            toggleQuick = requests::toggleQuick,
            setTyped = requests::setTyped,
            setOrder = requests::setOrder,
            setPaused = requests::setPaused,
            open = requests::open,
            close = requests::close,
            setExportFormat = requests::setExportFormat,
            setRedact = requests::setRedact,
            export = requests::export,
            openReplay = requests::openReplay,
            editReplay = requests::editReplay,
            sendReplay = requests::sendReplay,
            closeReplay = requests::closeReplay,
            exportAll = requests::exportAll,
            openSessions = requests::openSessions,
            closeSessions = requests::closeSessions,
            setSessionName = requests::setSessionName,
            saveSession = requests::saveSession,
            loadSession = requests::loadSession,
            deleteSession = requests::deleteSession,
            setShowConnections = requests::setShowConnections,
            openConnection = requests::openConnection,
            closeConnection = requests::closeConnection,
            openExchange = requests::openExchange,
            openConnectionOf = requests::openConnectionOf,
            decryptConnectionHost = requests::decryptConnectionHost,
            exportConnection = requests::exportConnection,
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
            takeImport = proxies::takeImport,
            useSubscription = proxies::useSubscription,
            updateSubscription = proxies::updateSubscription,
            renameSubscription = proxies::renameSubscription,
            setInterval = proxies::setInterval,
            removeSubscription = proxies::removeSubscription,
            openCheckup = proxies::openCheckup,
            runCheckup = proxies::runCheckup,
            closeCheckup = proxies::closeCheckup,
        ),
        rulesState = rulesState,
        rules = RulesActions(
            setRoutingMode = app::setRoutingMode,
            saveMyRules = rules::saveMyRules,
            selectGroupOutbound = rules::selectGroupOutbound,
            updateRuleSets = rules::updateRuleSets,
            setDnsPreset = app::setDnsPreset,
            saveDns = rules::saveDns,
            setBlockQuic = app::setBlockQuic,
            saveDataSource = rules::saveDataSource,
            removeDataSource = rules::removeDataSource,
            clearRuleHits = rules::clearRuleHits,
            testRoute = rules::testRoute,
        ),
        settingsState = settingsState,
        settings = settingsActions,
        termsVersion = termsVersion,
        logsState = logsState,
        trafficState = trafficState,
        activity = ActivityActions(
            setLevel = activity::setLevel,
            setModule = activity::setModule,
            setQuery = activity::setQuery,
            refreshLogs = { activity.refreshLogs() },
            copyLogs = { activity.copyLogs() },
            clearLogs = { activity.clearLogs() },
            setPeriod = activity::setPeriod,
            setView = activity::setView,
            clearTraffic = { activity.clearTraffic() },
        ),
        extensionsState = extensionsState,
        extensions = extensionsActions,
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
        pollLogs = {
            lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { activity.pollLogs() }
        },
        pollTraffic = {
            lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { activity.pollTraffic() }
        },
        pollExtensions = {
            lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { extensions.poll() }
        },
        pollRequests = {
            lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) { requests.poll() }
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

/**
 * The extensions page's actions: the system's security settings for installing the root certificate, and the
 * file picker whose file is imported as a plugin under its own name.
 */
@Composable
private fun rememberExtensionsActions(extensions: ExtensionsViewModel, snackbar: SnackbarHostState): ExtensionsActions {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val unreadable = stringResource(R.string.plugin_file_unreadable)
    val picker = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        scope.launch {
            val file = withContext(Dispatchers.IO) {
                try {
                    val name = context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
                        ?.use { cursor -> if (cursor.moveToFirst()) cursor.getString(0) else null }
                    val text = context.contentResolver.openInputStream(uri)?.use { it.bufferedReader().readText() }
                    text?.let { it to name.orEmpty() }
                } catch (error: IOException) {
                    null
                } catch (error: SecurityException) {
                    null
                }
            }
            if (file == null) snackbar.showSnackbar(unreadable) else extensions.importPluginText(file.first, file.second)
        }
    }
    return ExtensionsActions(
        enableDecryption = extensions::enableDecryption,
        disableDecryption = extensions::disableDecryption,
        confirmDecryption = extensions::confirmDecryption,
        openSecuritySettings = {
            try {
                context.startActivity(Intent(Settings.ACTION_SECURITY_SETTINGS))
            } catch (error: ActivityNotFoundException) {
                context.startActivity(Intent(Settings.ACTION_SETTINGS))
            }
        },
        addHost = { host, exclude -> extensions.addHost(host, exclude) },
        editHost = extensions::editHost,
        setHostEnabled = extensions::setHostEnabled,
        removeHost = extensions::removeHost,
        clearOutcomes = { extensions.clearOutcomes() },
        importPlugin = extensions::importPlugin,
        importPluginText = extensions::importPluginText,
        // Plugins come as .plugin, .sgmodule, .lpx, .conf or .txt, which carry no MIME type of their own.
        pickPluginFile = { picker.launch(arrayOf("*/*")) },
        setPluginEnabled = extensions::setPluginEnabled,
        removePlugin = extensions::removePlugin,
        inspect = { extensions.inspect(it) },
        closeInspector = extensions::closeInspector,
        editMapping = extensions::editMapping,
        closeMappingEditor = extensions::closeMappingEditor,
        saveMapping = extensions::saveMapping,
        setMappingEnabled = extensions::setMappingEnabled,
        removeMapping = extensions::removeMapping,
        editRedirect = extensions::editRedirect,
        closeRedirectEditor = extensions::closeRedirectEditor,
        saveRedirect = extensions::saveRedirect,
        setRedirectEnabled = extensions::setRedirectEnabled,
        removeRedirect = extensions::removeRedirect,
    )
}

/** The pages opened from 设置. */
enum class SettingsPage { Logs, Traffic }

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
    logsState: LogsState = LogsState(),
    trafficState: TrafficState = TrafficState(),
    activity: ActivityActions = ActivityActions(),
    extensionsState: ExtensionsState = ExtensionsState(),
    extensions: ExtensionsActions = ExtensionsActions(),
    debugExit: DebugExit = DebugExit.Direct,
    egress: EgressActions = EgressActions(),
    requestsState: RequestsState = RequestsState(),
    requests: RequestsActions = RequestsActions(),
    termsVersion: String = "",
    snackbar: SnackbarHostState = remember { SnackbarHostState() },
    pollProxies: suspend () -> Unit = {},
    pollRules: suspend () -> Unit = {},
    pollSettings: suspend () -> Unit = {},
    pollLogs: suspend () -> Unit = {},
    pollTraffic: suspend () -> Unit = {},
    pollExtensions: suspend () -> Unit = {},
    pollRequests: suspend () -> Unit = {},
    initial: Destination? = null,
    initialSettingsPage: SettingsPage? = null,
    initialEgressRules: Boolean = false,
) {
    var settingsPage by rememberSaveable { mutableStateOf(initialSettingsPage) }
    val closePage = { settingsPage = null }
    var egressRules by rememberSaveable { mutableStateOf(initialEgressRules) }
    val closeRules = { egressRules = false }
    val destinations = Destination.of(mode)
    var selected by rememberSaveable(mode) { mutableStateOf(initial?.takeIf { it in destinations } ?: destinations.first()) }
    // An `anixops://import` link, or 首页's import button, opens the import form where the nodes are: 代理, or 出口 in
    // debug mode.
    if (proxiesState.pendingImport != null) {
        LaunchedEffect(proxiesState.pendingImport) {
            egressRules = false
            selected = if (Destination.Proxies in destinations) Destination.Proxies else Destination.Egress
        }
    }
    // A failure must show on the page where it happened: 首页 has its card and debug mode its bar; elsewhere in proxy
    // mode (规则, 代理, 设置…) it comes up in the snackbar once, when it is new.
    val problem = state.problem()
    val onHome = selected == Destination.Home
    LaunchedEffect(problem) {
        if (problem != null && mode == AppMode.Proxy && !onHome) snackbar.showSnackbar(problem)
    }
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
        Column {
            Box(Modifier.weight(1f)) {
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
                    Destination.Settings -> when (settingsPage) {
                        SettingsPage.Logs -> {
                            BackTo(closePage)
                            LaunchedEffect(Unit) { pollLogs() }
                            LogsScreen(logsState, activity.copy(back = closePage))
                        }
                        SettingsPage.Traffic -> {
                            BackTo(closePage)
                            LaunchedEffect(Unit) { pollTraffic() }
                            TrafficScreen(trafficState, activity.copy(back = closePage))
                        }
                        null -> {
                            LaunchedEffect(Unit) { pollSettings() }
                            SettingsScreen(
                                mode,
                                version,
                                termsVersion,
                                state.snapshot,
                                settingsState,
                                settings.copy(
                                    setMode = onModeChange,
                                    openLogs = { settingsPage = SettingsPage.Logs },
                                    openTraffic = { settingsPage = SettingsPage.Traffic },
                                ),
                            )
                        }
                    }
                    Destination.Egress -> if (egressRules) {
                        BackTo(closeRules)
                        LaunchedEffect(Unit) { pollRules() }
                        RulesScreen(state.snapshot, state.busy, rulesState, rules, onBack = closeRules)
                    } else {
                        LaunchedEffect(Unit) { pollProxies() }
                        EgressScreen(
                            debugExit,
                            state.nodeCount,
                            proxiesState,
                            proxies,
                            egress.copy(openRules = { egressRules = true }),
                        )
                    }
                    Destination.Extensions, Destination.Rewrite -> {
                        LaunchedEffect(Unit) { pollExtensions() }
                        ExtensionsScreen(stringResource(selected.label), state.snapshot, extensionsState, extensions)
                    }
                    Destination.Requests -> {
                        LaunchedEffect(Unit) { pollRequests() }
                        when {
                            requestsState.sessions != null -> BackTo(requests.closeSessions)
                            requestsState.replay != null -> BackTo(requests.closeReplay)
                            requestsState.opened != null -> BackTo(requests.close)
                            requestsState.connection != null -> BackTo(requests.closeConnection)
                        }
                        RequestsScreen(
                            state,
                            requestsState,
                            requests.copy(openRewrite = { selected = Destination.Rewrite }),
                        )
                    }
                }
                SnackbarHost(snackbar, modifier = Modifier.align(Alignment.BottomCenter))
            }
            // Debug mode has no 首页: the connection switch and the last failure stay in a bar under every page.
            if (mode == AppMode.Debug) DebugConnectionBar(state, home)
        }
    }
}

/** The system back gesture closes a page opened from another; screenshots have no dispatcher to hook into. */
/** Why a link was refused, by the shared view's id. */
@StringRes
private fun linkReason(reason: String): Int = when (reason) {
    "invalid" -> R.string.link_invalid
    "not_app_link" -> R.string.link_not_app_link
    "too_long" -> R.string.link_too_long
    "missing_action" -> R.string.link_missing_action
    "missing_parameter" -> R.string.link_missing_parameter
    "unknown_mode" -> R.string.link_unknown_mode
    else -> R.string.link_unknown_action
}

@Composable
internal fun BackTo(close: () -> Unit) {
    if (LocalOnBackPressedDispatcherOwner.current != null) BackHandler(onBack = close)
}

package io.anixops.net.ui

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import io.anixops.net.core.CaptureExchange
import io.anixops.net.core.CaptureMessage
import io.anixops.net.core.CaptureSession
import io.anixops.net.core.CaptureStatus
import io.anixops.net.core.Core
import io.anixops.net.core.CoreException
import io.anixops.net.core.FilterError
import io.anixops.net.core.HeaderField
import io.anixops.net.core.MappingText
import io.anixops.net.core.ReplayRequest
import io.anixops.net.core.ReplayResponse
import io.anixops.net.core.RequestFeed
import io.anixops.net.core.RequestKey
import io.anixops.net.core.RequestPage
import io.anixops.net.core.RequestRow
import kotlinx.coroutines.channels.Channel
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.receiveAsFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock

/** The quick filters by id, in the order the chips show; `view.combine_filter` knows their conditions. */
val QuickFilters = listOf("errors", "json", "images", "h2", "large")

/** The columns the list can be ordered by (`view.request_rows`); `seq` is when requests started. */
val RequestOrders = listOf("seq", "status", "host", "size", "duration")

/** The opened request: its row from the list, and its exchange once read (null until then, or once dropped). */
data class OpenedRequest(
    val row: RequestRow,
    val exchange: CaptureExchange? = null,
    /** Capture no longer holds it (cleared, or pushed out by newer traffic). */
    val gone: Boolean = false,
    val requestBody: BodyText? = null,
    val responseBody: BodyText? = null,
)

/** The formats one request exports to, by id, with their names; the code formats are `code` in that language. */
val ExportFormats = listOf(
    "curl" to "cURL",
    "python" to "Python (requests)",
    "javascript" to "JavaScript (fetch)",
    "swift" to "Swift (URLSession)",
    "kotlin" to "Kotlin (OkHttp)",
    "go" to "Go (net/http)",
    "postman" to "Postman",
    "insomnia" to "Insomnia",
    "har" to "HAR",
)

/** The languages of the `code` export. */
private val CodeLanguages = setOf("python", "javascript", "swift", "kotlin", "go")

/**
 * The replay editor over the opened request: what will be sent, and what came back. [body] is the request body as
 * text, or null when it is bytes that are not text and go as captured ([binaryBase64]).
 */
data class ReplayDraft(
    val method: String,
    val url: String,
    val headers: String,
    val body: String?,
    val binaryBase64: String = "",
    /** Capture kept only the start of the body, so the copy sends only that. */
    val cut: Boolean = false,
    val sending: Boolean = false,
    val answer: ReplayResponse? = null,
    val answerBody: BodyText? = null,
)

/** The saved capture sessions page, opened from the requests page's menu. */
data class SessionsPage(
    val loaded: Boolean = false,
    val sessions: List<CaptureSession> = emptyList(),
    /** What keeps the sessions' key on this device (`keystore` on Android). */
    val protection: String = "",
    val retentionDays: Int = 7,
    /** Why the list could not be read. */
    val failure: String? = null,
    /** The name the next save gets; blank saves it under the time. */
    val name: String = "",
    val working: Boolean = false,
)

/** The requests page: capture, the filters and order, the rows shown, and the request opened. */
data class RequestsState(
    /** Null while the engine is not running. */
    val capture: CaptureStatus? = null,
    val quick: Set<String> = emptySet(),
    val typed: String = "",
    val order: String = "seq",
    val descending: Boolean = true,
    val paused: Boolean = false,
    val rows: List<RequestRow> = emptyList(),
    val error: FilterError? = null,
    val loaded: Boolean = false,
    val busy: Boolean = false,
    val opened: OpenedRequest? = null,
    val exportFormat: String = "curl",
    /** Exports mask cookies, tokens and passwords unless this is turned off. */
    val redact: Boolean = true,
    val replay: ReplayDraft? = null,
    val sessions: SessionsPage? = null,
    /** What is left before HTTPS requests show (`view.decrypt_setup`); empty once they can. */
    val setup: List<String> = emptyList(),
)

/** A sentence for the snackbar, or where an export was saved. */
sealed interface RequestsEvent {
    data class Message(val text: String) : RequestsEvent

    data class Saved(val location: String) : RequestsEvent

    /** A saved session was saved, loaded or deleted. */
    data class Session(val change: SessionChange, val session: CaptureSession) : RequestsEvent
}

enum class SessionChange { Saved, Loaded, Deleted }

class RequestsViewModel(private val core: Core) : ViewModel() {
    private val _state = MutableStateFlow(RequestsState())
    val state: StateFlow<RequestsState> = _state.asStateFlow()

    private val _events = Channel<RequestsEvent>(Channel.BUFFERED)
    val events: Flow<RequestsEvent> = _events.receiveAsFlow()

    /** What has been read through the current filter; a new filter starts it over. */
    private var feed = RequestFeed()

    /** The filter [feed] was read through. */
    private var filter = ""

    private val reading = Mutex()

    /** Whether the app is connected and decrypting, which the checklist needs; the app's own state says. */
    private var connected = false
    private var decrypting = false

    /** Reads capture and the list every [POLL_MILLIS] until cancelled; the requests page runs it while it shows. */
    suspend fun poll() {
        while (true) {
            read()
            delay(POLL_MILLIS)
        }
    }

    /** The connection and decryption as the app shows them, for the checklist. */
    fun setSetupInputs(connected: Boolean, decrypting: Boolean) {
        if (connected == this.connected && decrypting == this.decrypting) return
        this.connected = connected
        this.decrypting = decrypting
        viewModelScope.launch { read() }
    }

    fun toggleQuick(id: String) = refilter { it.copy(quick = if (id in it.quick) it.quick - id else it.quick + id) }

    fun setTyped(text: String) = refilter { it.copy(typed = text) }

    fun setOrder(order: String) {
        _state.update {
            // Choosing the column already in use turns the order around.
            if (it.order == order) it.copy(descending = !it.descending) else it.copy(order = order, descending = true)
        }
        viewModelScope.launch { read() }
    }

    fun setPaused(paused: Boolean) {
        _state.update { it.copy(paused = paused) }
        if (!paused) viewModelScope.launch { read() }
    }

    fun setCapture(enabled: Boolean) = perform { core.setCapture(enabled) }

    fun clear() = perform {
        core.clearCapture().also {
            reading.withLock { feed = RequestFeed() }
            _state.update { state -> state.copy(rows = emptyList(), opened = null) }
        }
    }

    fun open(row: RequestRow) {
        _state.update { it.copy(opened = OpenedRequest(row)) }
        viewModelScope.launch { readOpened(row.key) }
    }

    fun close() = _state.update { it.copy(opened = null, replay = null) }

    fun setExportFormat(format: String) = _state.update { it.copy(exportFormat = format) }

    fun setRedact(redact: Boolean) = _state.update { it.copy(redact = redact) }

    /** Saves the opened request to Downloads in the chosen format and says where. */
    fun export() {
        val state = _state.value
        val row = state.opened?.row ?: return
        val format = state.exportFormat
        val code = format in CodeLanguages
        viewModelScope.launch {
            try {
                val saved = core.exportExchange(
                    row.connectionId,
                    row.index,
                    if (code) "code" else format,
                    language = format.takeIf { code },
                    redact = state.redact,
                )
                _events.send(RequestsEvent.Saved(saved.location))
            } catch (error: CoreException) {
                _events.send(RequestsEvent.Message(error.message.orEmpty()))
            }
        }
    }

    /** Opens the replay editor on a copy of the opened request; HTTP/2 pseudo-headers are left out. */
    fun openReplay() {
        val exchange = _state.value.opened?.exchange ?: return
        val request = exchange.request
        val headers = request.headers.filter { it.size == 2 && !it[0].startsWith(":") }.map { HeaderField(it[0], it[1]) }
        val bytes = MappingText.bytes(request.body.dataBase64)
        val text = MappingText.text(bytes)
        _state.update {
            it.copy(
                replay = ReplayDraft(
                    method = exchange.method,
                    url = exchange.url,
                    headers = MappingText.headerLines(headers),
                    body = text,
                    binaryBase64 = if (text == null) request.body.dataBase64 else "",
                    cut = request.body.truncated,
                ),
            )
        }
    }

    fun editReplay(change: (ReplayDraft) -> ReplayDraft) = _state.update { state -> state.copy(replay = state.replay?.let(change)) }

    fun closeReplay() = _state.update { it.copy(replay = null) }

    fun sendReplay() {
        val draft = _state.value.replay ?: return
        if (draft.sending || draft.url.isBlank()) return
        editReplay { it.copy(sending = true, answer = null, answerBody = null) }
        viewModelScope.launch {
            val request = ReplayRequest(
                method = draft.method.trim().ifEmpty { "GET" },
                url = draft.url.trim(),
                headers = MappingText.parseHeaderLines(draft.headers).map { listOf(it.name, it.value) },
                bodyBase64 = draft.body?.let { MappingText.base64(it) } ?: draft.binaryBase64,
            )
            try {
                val answer = core.replay(request)
                val body = body(answer.message)
                editReplay { it.copy(sending = false, answer = answer, answerBody = body) }
            } catch (error: CoreException) {
                editReplay { it.copy(sending = false) }
                _events.send(RequestsEvent.Message(error.message.orEmpty()))
            }
        }
    }

    /** Saves everything capture holds (`har` or `pcapng`) to Downloads and says where. */
    fun exportAll(format: String) {
        viewModelScope.launch {
            try {
                _events.send(RequestsEvent.Saved(core.exportCapture(format).location))
            } catch (error: CoreException) {
                _events.send(RequestsEvent.Message(error.message.orEmpty()))
            }
        }
    }

    fun openSessions() {
        _state.update { it.copy(sessions = SessionsPage()) }
        viewModelScope.launch { readSessions() }
    }

    fun closeSessions() = _state.update { it.copy(sessions = null) }

    fun setSessionName(name: String) = editSessions { it.copy(name = name) }

    /** Seals what capture holds into a new session under the name typed (or the time). */
    fun saveSession() = sessionCommand { page ->
        val saved = core.saveCaptureSession(page.name)
        editSessions { it.copy(name = "") }
        RequestsEvent.Session(SessionChange.Saved, saved)
    }

    /** Replaces what capture holds with [session], turns live capture off and goes back to the list. */
    fun loadSession(session: CaptureSession) = sessionCommand {
        val status = core.loadCaptureSession(session.session)
        reading.withLock { feed = RequestFeed() }
        _state.update { it.copy(capture = status, rows = emptyList(), opened = null, sessions = null) }
        RequestsEvent.Session(SessionChange.Loaded, session)
    }

    fun deleteSession(session: CaptureSession) = sessionCommand {
        core.deleteCaptureSession(session.session)
        RequestsEvent.Session(SessionChange.Deleted, session)
    }

    /** One session command at a time; then the list (or, once a session is loaded, the requests) is read again. */
    private fun sessionCommand(command: suspend (SessionsPage) -> RequestsEvent) {
        val page = _state.value.sessions ?: return
        if (page.working) return
        editSessions { it.copy(working = true) }
        viewModelScope.launch {
            val event = try {
                command(page)
            } catch (error: CoreException) {
                RequestsEvent.Message(error.message.orEmpty())
            }
            editSessions { it.copy(working = false) }
            _events.send(event)
            if (_state.value.sessions != null) readSessions() else read()
        }
    }

    private suspend fun readSessions() {
        val answer = try {
            core.captureSessions()
        } catch (error: CoreException) {
            editSessions { it.copy(loaded = true, failure = error.message.orEmpty()) }
            return
        }
        editSessions {
            it.copy(
                loaded = true,
                sessions = answer.sessions,
                protection = answer.protection,
                retentionDays = answer.retentionDays,
                failure = null,
            )
        }
    }

    private fun editSessions(change: (SessionsPage) -> SessionsPage) =
        _state.update { state -> state.copy(sessions = state.sessions?.let(change)) }

    private fun refilter(change: (RequestsState) -> RequestsState) {
        _state.update(change)
        viewModelScope.launch { read() }
    }

    private fun perform(command: suspend () -> CaptureStatus) {
        if (_state.value.busy) return
        _state.update { it.copy(busy = true) }
        viewModelScope.launch {
            val status = try {
                command()
            } catch (error: CoreException) {
                _events.send(RequestsEvent.Message(error.message.orEmpty()))
                null
            }
            _state.update { it.copy(busy = false, capture = status ?: it.capture) }
            read()
        }
    }

    /** One read at a time: the poll and a changed filter both read, and each carries on from [feed]. */
    private suspend fun read() = reading.withLock { readLocked() }

    private suspend fun readLocked() {
        val capture = quiet { core.captureStatus() }
        val domains = quiet { core.decryptHosts().size } ?: 0
        val setup = quiet { core.decryptSetup(connected, decrypting, domains, capturing = capture?.enabled == true) }
        _state.update { it.copy(capture = capture, setup = setup ?: it.setup) }
        val asked = _state.value
        if (capture == null) {
            _state.update { it.copy(loaded = true) }
            return
        }
        val wanted = quiet { core.combineFilter(QuickFilters.filter { it in asked.quick }, asked.typed) } ?: return
        if (wanted != filter) {
            filter = wanted
            feed = RequestFeed()
        }
        // Paused, the list keeps what it has; a new order still applies to it.
        val page = if (asked.paused) {
            quiet { RequestPage(feed, core.requestRows(feed.rows, asked.order, asked.descending), _state.value.error) }
        } else {
            quiet { core.requests(feed, filter, asked.order, asked.descending) }
        } ?: return
        feed = page.feed
        _state.update {
            if (it.order != asked.order || it.descending != asked.descending) {
                it
            } else {
                it.copy(rows = page.shown, error = page.error, loaded = true)
            }
        }
        // The opened request fills in as its response arrives.
        val opened = _state.value.opened ?: return
        val row = page.shown.firstOrNull { it.key == opened.row.key }
        if (row != null && !opened.gone && (opened.exchange == null || row != opened.row)) {
            _state.update { state ->
                val current = state.opened
                if (current != null && current.row.key == row.key) state.copy(opened = current.copy(row = row)) else state
            }
            readOpened(row.key)
        }
    }

    private suspend fun readOpened(key: RequestKey) {
        val exchange = try {
            core.captureExchange(key)
        } catch (error: CoreException) {
            return
        }
        val requestBody = exchange?.let { body(it.request) }
        val responseBody = exchange?.let { body(it.response) }
        _state.update { state ->
            val opened = state.opened?.takeIf { it.row.key == key } ?: return@update state
            if (exchange == null) return@update state.copy(opened = opened.copy(gone = true))
            state.copy(opened = opened.copy(exchange = exchange, requestBody = requestBody, responseBody = responseBody))
        }
    }

    /** A body to show: its Content-Encoding undone by the core, or as captured when that cannot be done. */
    private suspend fun body(message: CaptureMessage): BodyText {
        val body = message.body
        val type = message.header("content-type")
        val encoding = message.header("content-encoding")?.trim().orEmpty()
        if (encoding.isEmpty() || encoding.equals("identity", ignoreCase = true) || body.dataBase64.isEmpty()) {
            return BodyText.of(body.dataBase64, type, body.truncated)
        }
        return try {
            val decoded = core.decodeBody(body, encoding)
            BodyText.of(decoded.dataBase64, type, body.truncated || decoded.truncated)
        } catch (error: CoreException) {
            BodyText.of(body.dataBase64, type, body.truncated, undecoded = true)
        }
    }

    /** A poll that fails keeps what the page shows rather than repeating the refusal every second. */
    private suspend fun <T> quiet(read: suspend () -> T): T? = try {
        read()
    } catch (error: CoreException) {
        null
    }

    companion object {
        private const val POLL_MILLIS = 1_000L
    }
}

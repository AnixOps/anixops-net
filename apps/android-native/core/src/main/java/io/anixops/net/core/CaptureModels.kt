package io.anixops.net.core

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.longOrNull

/** Whether the engine keeps captured traffic, and how much of its budget it holds (`capture`). */
@Serializable
data class CaptureStatus(
    val enabled: Boolean = false,
    @SerialName("max_total_bytes") val maxTotalBytes: Long = 0,
    @SerialName("used_bytes") val usedBytes: Long = 0,
    val connections: Int = 0,
)

/**
 * One decrypted request as `view.request_rows` lists it: the engine's row (`capture_requests`) with its [path] and
 * status [tone] (`success`, `warning`, `danger`, `pending` or `neutral`) added.
 */
@Serializable
data class RequestRow(
    val seq: Long = 0,
    @SerialName("connection_id") val connectionId: Long = 0,
    /** Its place in the connection's exchanges. */
    val index: Int = 0,
    val method: String = "",
    val url: String = "",
    val host: String = "",
    val status: Int? = null,
    /** `answered_locally`, `streaming` or `too_large`. */
    val note: String? = null,
    @SerialName("http_version") val httpVersion: String = "",
    @SerialName("started_at_ms") val startedAtMs: Long = 0,
    @SerialName("ended_at_ms") val endedAtMs: Long? = null,
    @SerialName("request_size") val requestSize: Long = 0,
    @SerialName("response_size") val responseSize: Long = 0,
    @SerialName("response_content_type") val responseContentType: String? = null,
    /** `in_progress` until the response ends, then `done`. */
    val state: String? = null,
    @SerialName("message_count") val messageCount: Int? = null,
    val path: String = "",
    val tone: String = "neutral",
) {
    /** The request's place in the capture, the same while it is under way and after. */
    val key: RequestKey get() = RequestKey(connectionId, index)

    val durationMs: Long? get() = endedAtMs?.let { it - startedAtMs }
}

/** A request by its connection and its place there. */
@Serializable
data class RequestKey(val connection: Long, val index: Int)

/**
 * What the request list has read so far: the engine's rows as they came (so fields this app does not know stay on
 * them), and where the next read starts.
 */
data class RequestFeed(
    val rows: JsonArray = JsonArray(emptyList()),
    val lastSeq: Long = 0,
    /** Null until the first answer, and for a kernel without `since_rev`. */
    val lastRev: Long? = null,
)

/**
 * Whether the engine behind [feed] is a new one: a changed setting or a reconnect restarts it, and its counters
 * ([lastSeq], [lastRev] of an answer) start again below what the list has read. The list then starts over, or the old
 * cursor would hide every new request and old rows would share keys with new ones.
 */
fun engineRestarted(feed: RequestFeed, lastSeq: Long?, lastRev: Long?): Boolean {
    val readRev = feed.lastRev
    if (lastRev != null && readRev != null) return lastRev < readRev
    return lastSeq != null && lastSeq < feed.lastSeq
}

/*
 * The cursor of the next read, worked out here rather than through `view.next_since` / `view.next_rev`: those take
 * the whole list (up to 2000 rows) across JNI every second only to return a number. They follow client-view's
 * `next_since` and `next_rev` (crates/client-view/src/capture.rs) line by line, and ViewModelsTest checks them
 * against that crate's samples (crates/client-view/fixtures/cases.json).
 */

private fun JsonObject.unsigned(name: String): Long? =
    (get(name) as? JsonPrimitive)?.takeIf { !it.isString }?.longOrNull?.takeIf { it >= 0 }

private fun JsonObject.signed(name: String): Long? = (get(name) as? JsonPrimitive)?.takeIf { !it.isString }?.longOrNull

private fun JsonObject.string(name: String): String? = (get(name) as? JsonPrimitive)?.takeIf { it.isString }?.content

private fun JsonObject.seq(): Long = unsigned("seq") ?: 0L

/** Whether the engine may still fill in this request's response; see client-view `pending`. */
private fun JsonObject.pending(): Boolean = when (val state = string("state")) {
    null -> signed("status") == null && signed("ended_at_ms") == null && string("note").isNullOrEmpty()
    else -> state == "in_progress"
}

/**
 * Where the next read by `since_seq` starts (kernels without `since_rev`): before the oldest request still waiting
 * for its response, so it comes back with the response, or [lastSeq], after the newest one seen.
 */
fun nextSince(rows: JsonArray, lastSeq: Long): Long {
    for (row in rows) {
        val request = row as? JsonObject ?: continue
        val seq = request.seq()
        if (seq > 0 && request.pending()) return seq - 1
    }
    return lastSeq
}

/**
 * The `since_rev` of the next read after [answer], or null for a kernel without `since_rev`: a full page ([page]
 * rows) carries on from its last row, since the engine lists in the order things changed; otherwise the read starts
 * after the newest change.
 */
fun nextRev(current: Long?, answer: JsonElement, page: Int): Long? {
    val read = answer as? JsonObject ?: return null
    val lastRev = read.unsigned("last_rev") ?: return null
    val requests = read["requests"] as? JsonArray ?: JsonArray(emptyList())
    val last = if (page > 0 && requests.size >= page) (requests.lastOrNull() as? JsonObject)?.unsigned("rev") else null
    return maxOf(current ?: 0L, last ?: lastRev)
}

/**
 * Whether merging [answer] into [rows] (`view.merge_requests`) would give [rows] back unchanged: the engine sent no
 * request and pushed out none of the rows. [rows] come from an earlier merge, so they are already in order and within
 * the list's limit. The list then needs neither the merge nor ordering again.
 */
fun mergeChangesNothing(rows: JsonArray, answer: JsonElement): Boolean {
    val read = answer as? JsonObject ?: return false
    val requests = read["requests"]
    if (requests != null && requests !is JsonArray) return false
    if ((requests as? JsonArray)?.isNotEmpty() == true) return false
    val dropped = read.unsigned("dropped_before_seq") ?: 0L
    return rows.none { row ->
        val seq = (row as? JsonObject)?.seq() ?: 0L
        seq in 1..dropped
    }
}

/** One read of the list: what was read, the rows to show in order, and why the engine refused the filter. */
data class RequestPage(
    val feed: RequestFeed,
    val shown: List<RequestRow>,
    val error: FilterError? = null,
)

/** The engine refused the filter expression: `invalid_filter` with the position of the mistake when it says. */
@Serializable
data class FilterError(val code: String = "", val at: Int? = null, val message: String? = null)

/** A captured body: its size, whether capture kept all of it, and the bytes it kept. */
@Serializable
data class CaptureBody(
    val size: Long = 0,
    val truncated: Boolean = false,
    @SerialName("data_base64") val dataBase64: String = "",
)

/** One side of an exchange: its headers as name and value pairs, and its body. */
@Serializable
data class CaptureMessage(
    val headers: List<List<String>> = emptyList(),
    val body: CaptureBody = CaptureBody(),
) {
    /** The value of the first header named [name], in any case. */
    fun header(name: String): String? =
        headers.firstOrNull { it.size == 2 && it[0].equals(name, ignoreCase = true) }?.get(1)
}

/** One decrypted request and its response (`capture_exchange`). */
@Serializable
data class CaptureExchange(
    @SerialName("started_at_ms") val startedAtMs: Long = 0,
    @SerialName("ended_at_ms") val endedAtMs: Long? = null,
    val method: String = "",
    val url: String = "",
    val status: Int? = null,
    val note: String? = null,
    val request: CaptureMessage = CaptureMessage(),
    val response: CaptureMessage = CaptureMessage(),
    val state: String? = null,
    @SerialName("rules_skipped") val rulesSkipped: Boolean = false,
)

/** A body with its Content-Encoding undone (`decode_body`). */
@Serializable
data class DecodedBody(
    @SerialName("data_base64") val dataBase64: String = "",
    val truncated: Boolean = false,
)

/** An edited copy of a captured request, sent again through the engine (`replay_request`). */
@Serializable
data class ReplayRequest(
    val method: String,
    val url: String,
    val headers: List<List<String>>,
    /** Empty for no body. */
    val bodyBase64: String = "",
)

/** What a replayed request got back; the body is cut at 1 MiB. */
@Serializable
data class ReplayResponse(
    val status: Int = 0,
    val headers: List<List<String>> = emptyList(),
    val bodyBase64: String = "",
    val truncated: Boolean = false,
    val elapsedMs: Long = 0,
) {
    val message: CaptureMessage get() = CaptureMessage(headers, CaptureBody(0, truncated, bodyBase64))
}

/** Where an export was saved (`export_capture`). */
@Serializable
data class SavedFile(val location: String = "")

/** One saved capture session (`capture_session` list); [name] is empty when it was saved without one. */
@Serializable
data class CaptureSession(
    val session: String = "",
    val name: String? = null,
    @SerialName("saved_at_ms") val savedAtMs: Long = 0,
    @SerialName("expires_at_ms") val expiresAtMs: Long = 0,
    val size: Long = 0,
    val connections: Int = 0,
    val requests: Int = 0,
    /** Sealed with another key (a reinstall): it cannot be opened and expires on its own. */
    val unreadable: Boolean = false,
)

/**
 * The saved sessions, newest first, with what keeps their key on this device ([protection]: `keystore` on Android)
 * and how many days they are kept.
 */
@Serializable
data class CaptureSessions(
    val sessions: List<CaptureSession> = emptyList(),
    val protection: String = "",
    @SerialName("retention_days") val retentionDays: Int = 7,
)

/** The answer to saving a session. */
@Serializable
data class SavedSession(val session: CaptureSession = CaptureSession())

/** What is left before HTTPS requests can show (`view.decrypt_setup`), in order. */
@Serializable
data class DecryptSetup(val steps: List<String> = emptyList())

/** The answer to deleting a session. */
@Serializable
data class DeletedSession(val deleted: Boolean = false)

/** One direction of a captured connection: bytes seen, bytes kept, and whether keeping stopped partway. */
@Serializable
data class CaptureDirection(
    val seen: Long = 0,
    val captured: Long = 0,
    val truncated: Boolean = false,
)

/** One read or write of a captured connection; [dir] is `upload` or `download`. */
@Serializable
data class CaptureChunk(
    val dir: String = "",
    @SerialName("at_ms") val atMs: Long = 0,
    @SerialName("data_base64") val dataBase64: String = "",
)

/** What capture holds of one connection (`capture_connection`): its bytes and its decrypted requests. */
@Serializable
data class CaptureConnection(
    val id: Long = 0,
    val network: String = "",
    val host: String? = null,
    val destination: String = "",
    @SerialName("started_at_ms") val startedAtMs: Long = 0,
    @SerialName("ended_at_ms") val endedAtMs: Long? = null,
    val decrypted: Boolean = false,
    val upload: CaptureDirection = CaptureDirection(),
    val download: CaptureDirection = CaptureDirection(),
    val chunks: List<CaptureChunk> = emptyList(),
    val http: List<CaptureExchange> = emptyList(),
)

/** The answer to `capture_connection`: null once capture no longer holds the connection. */
@Serializable
data class CaptureConnectionAnswer(val capture: CaptureConnection? = null)

/**
 * A connection's host against the decrypt list (`view.decrypt_hosts`): whether decryption can be turned on for it,
 * the name the list would keep, and whether an entry already covers it.
 */
@Serializable
data class DecryptHost(
    val decryptable: Boolean = false,
    val host: String? = null,
    val covered: Boolean = false,
    /** An exclusion names it: it is never decrypted, whatever else lists it. */
    val excluded: Boolean = false,
)

/** One line of the decryption list (`list_decrypt_domains`). A paused line stays listed but does not apply. */
@Serializable
data class DecryptDomain(
    val pattern: String,
    val exclude: Boolean = false,
    val enabled: Boolean = true,
)

/** One domain of the engine's `mitm_hosts` answer: decrypted and failed connections, and the last failure's code. */
@Serializable
data class MitmHostOutcome(
    val host: String,
    val decrypted: Long = 0,
    val failed: Long = 0,
    @SerialName("last_failure") val lastFailure: String? = null,
    @SerialName("last_failure_at_ms") val lastFailureAtMs: Long? = null,
    @SerialName("last_decrypted_at_ms") val lastDecryptedAtMs: Long? = null,
) {
    /** The last connection failed: nothing was decrypted since the last failure. */
    val failing: Boolean
        get() = failed > 0 && lastFailureAtMs != null && (lastDecryptedAtMs == null || lastDecryptedAtMs <= lastFailureAtMs)
}

/** The `mitm_hosts` answer; `enabled` is false while the engine runs without decryption. */
@Serializable
data class MitmHosts(val enabled: Boolean = false, val hosts: List<MitmHostOutcome> = emptyList())

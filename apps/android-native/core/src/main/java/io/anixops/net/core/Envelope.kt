package io.anixops.net.core

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject

/**
 * A command the Rust client refused, or an answer that could not be read; [message] is what to show. [Core] words a
 * refusal in the app's language ([CoreTexts]): then [code] is the refusal's code, if it had one, and [detail] the
 * client's own English.
 */
class CoreException(message: String, val detail: String? = null, val code: String? = null) : Exception(message)

/**
 * The `{"ok": answer}` / `{"error": "..."}` envelope both directions of the bridge use: Rust answers commands in
 * it, and Kotlin answers the Rust side's platform calls in it.
 */
object Envelope {
    /** Lenient on what Rust adds later; strict on what Kotlin declares. */
    val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
        encodeDefaults = true
    }

    /** The answer inside [text], or [CoreException] with the refusal. */
    fun read(text: String): JsonElement {
        val envelope = try {
            json.parseToJsonElement(text).jsonObject
        } catch (error: IllegalArgumentException) {
            throw CoreException("Unreadable answer: ${error.message}")
        }
        envelope["error"]?.let { error ->
            val message = (error as? JsonPrimitive)?.takeIf { it.isString }?.content ?: error.toString()
            throw CoreException(message)
        }
        return envelope["ok"] ?: throw CoreException("The answer has neither ok nor error")
    }

    fun ok(answer: JsonElement = JsonNull): String = buildJsonObject { put("ok", answer) }.toString()

    fun error(message: String): String = buildJsonObject { put("error", JsonPrimitive(message)) }.toString()

    /** Runs [answer] and wraps what it returns, or what it throws, in the envelope. */
    inline fun answer(answer: () -> JsonElement): String = try {
        ok(answer())
    } catch (error: Exception) {
        error(error.message ?: error.javaClass.simpleName)
    }

    val empty = JsonObject(emptyMap())
}

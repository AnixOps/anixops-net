package io.anixops.net.core

/**
 * The words of the client's codes in the app's language: the app's string resources (`CodeTexts` in the app module),
 * the same words as the desktop window's (`operation-text.ts`, `error-text.ts`). [Core] uses them so no screen shows
 * the client's English: a result by its `code`, a refusal by the code it starts with (`<code>: <English>`,
 * crates/client-core/src/refusal.rs), anything else as "the operation did not finish" with the English after it.
 */
interface CoreTexts {
    /** The words for [code] with each `{name}` of [params] filled in, or null when the app has none. */
    fun text(code: String, params: Map<String, String> = emptyMap()): String?

    /** [first], then [then], as two sentences. */
    fun join(first: String, then: String): String

    /** A refusal with no code the app knows: [detail] is the client's English. */
    fun failed(detail: String): String

    /** An answer the app cannot read (a client of another version). */
    fun unreadable(): String

    /** No words of its own: the client's English throughout. For tests and the JVM-only screenshots. */
    object English : CoreTexts {
        override fun text(code: String, params: Map<String, String>): String? = null

        override fun join(first: String, then: String): String = "${first.trimEnd('.')}. $then"

        override fun failed(detail: String): String = detail

        override fun unreadable(): String = "The client gave an answer this app cannot read"
    }

    companion object {
        private val codePattern = Regex("""^([a-z][a-z0-9_]*(?:\.[a-z0-9_]+)+): ([\s\S]*)$""")

        /** The code a refusal starts with and its English, or null for a plain one (as `refusal::split`). */
        fun split(refusal: String): Pair<String, String>? =
            codePattern.matchEntire(refusal)?.let { it.groupValues[1] to it.groupValues[2] }

        /** Fills each `{name}` of [text]. */
        fun fill(text: String, params: Map<String, String>): String =
            params.entries.fold(text) { result, (name, value) -> result.replace("{$name}", value) }
    }
}

/** [result] in the app's words when it knows its codes; otherwise as the client sent it. */
internal fun CoreTexts.word(result: OperationResult): OperationResult {
    val code = result.code ?: return result
    val first = text(code, result.params) ?: return result
    val worded = when (val follow = result.follow) {
        null -> first
        else -> text(follow)?.let { join(first.trimEnd('.', '。'), it) } ?: return result
    }
    return result.copy(message = listOfNotNull(worded, result.note).joinToString(" "))
}

/** [result] in the app's words when it knows its code. */
internal fun CoreTexts.word(result: SavedFileResult): SavedFileResult {
    val code = result.code ?: return result
    val worded = text(code, result.params) ?: return result
    return result.copy(message = worded)
}

/** A refusal in the app's words: by its code, or as what did not finish with the client's English after it. */
internal fun CoreTexts.refusal(error: CoreException): CoreException {
    val raw = error.message.orEmpty()
    val (code, english) = CoreTexts.split(raw) ?: (null to raw)
    val known = code?.let { text(it) }
    return CoreException(known ?: failed(english.ifEmpty { raw }), detail = raw, code = code)
}

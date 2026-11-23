package io.anixops.net.ui

import io.anixops.net.core.OperationResult

/**
 * Whether a plugin's decryption list reaches every site: `*`, `*.*`, or every name under a top-level domain
 * (`*.com`). The review before a plugin is turned on says so plainly. A leading `-` marks an exclusion, which
 * decrypts nothing. The same rule as the desktop window's `plugin-review.ts`.
 */
fun decryptsBroadly(hosts: List<String>): Boolean = hosts.any { host ->
    val pattern = host.trim()
    when {
        pattern.startsWith("-") -> false
        pattern == "*" || pattern == "*.*" -> true
        else -> BROAD_PATTERN.matches(pattern)
    }
}

private val BROAD_PATTERN = Regex("""^\*\.[^.*]+$""")

/**
 * The plugin an import left off for the user to look at before it runs anything: the id the client names in a
 * `plugins.imported_off` answer, or null when the import kept a plugin on (an unchanged one the user had on).
 */
fun pluginToReview(result: OperationResult): String? =
    if (result.code == "plugins.imported_off") result.params["id"]?.takeIf { it.isNotEmpty() } else null

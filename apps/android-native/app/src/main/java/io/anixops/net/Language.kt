package io.anixops.net

import java.util.Locale

/**
 * The client's code for the language of the texts it makes itself, the notifications (`i18n.ts` `Language`):
 * Traditional Chinese for a Traditional script or Taiwan, Hong Kong and Macau, Simplified for other Chinese, and
 * English for everything else, as the app's own resources fall back.
 */
fun clientLanguage(locale: Locale): String = when {
    locale.language != "zh" -> "en"
    locale.script == "Hant" || locale.country in setOf("TW", "HK", "MO") -> "zh-Hant"
    else -> "zh"
}

package io.anixops.net.ui

import io.anixops.net.R
import io.anixops.net.clientLanguage
import java.util.Locale
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config

/**
 * English is the fallback, Simplified and Traditional Chinese are picked by script, and the notifications the client
 * writes follow the same choice (finding android-09).
 */
@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35])
class LanguageTest {
    private fun home() = RuntimeEnvironment.getApplication().getString(R.string.nav_home)

    @Test
    @Config(qualifiers = "ru")
    fun russianReadsEnglish() = assertEquals("Home", home())

    @Test
    @Config(qualifiers = "ja")
    fun japaneseReadsEnglish() = assertEquals("Home", home())

    @Test
    @Config(qualifiers = "fa")
    fun persianReadsEnglish() = assertEquals("Home", home())

    @Test
    @Config(qualifiers = "en")
    fun englishReadsEnglish() = assertEquals("Home", home())

    @Test
    @Config(qualifiers = "zh-rCN")
    fun mainlandChineseReadsSimplified() = assertEquals("首页", home())

    @Test
    @Config(qualifiers = "zh-rSG")
    fun singaporeChineseReadsSimplified() = assertEquals("首页", home())

    @Test
    @Config(qualifiers = "zh-rTW")
    fun taiwanReadsTraditional() = assertEquals("首頁", home())

    @Test
    @Config(qualifiers = "zh-rHK")
    fun hongKongReadsTraditional() = assertEquals("首頁", home())

    @Test
    @Config(qualifiers = "zh-rMO")
    fun macauReadsTraditional() = assertEquals("首頁", home())

    @Test
    @Config(qualifiers = "b+zh+Hant")
    fun traditionalScriptReadsTraditional() = assertEquals("首頁", home())

    @Test
    fun notificationsFollowTheSameLanguages() {
        assertEquals("en", clientLanguage(Locale.forLanguageTag("ru-RU")))
        assertEquals("en", clientLanguage(Locale.forLanguageTag("en-US")))
        assertEquals("zh", clientLanguage(Locale.forLanguageTag("zh-CN")))
        assertEquals("zh-Hant", clientLanguage(Locale.forLanguageTag("zh-HK")))
        assertEquals("zh-Hant", clientLanguage(Locale.forLanguageTag("zh-Hant-SG")))
    }
}

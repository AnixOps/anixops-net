package io.anixops.net.ui

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PrivacyTest {
    private val sample = """
        # Privacy Policy / 隐私政策

        Effective date / 生效日期：2026-11-23

        [English](#english) · [中文](#中文)

        ## English

        ### What we collect

        Nothing by default.

        ## 中文

        ### 我们收集什么

        默认不收集任何数据。
    """.trimIndent()

    @Test
    fun showsOnlyTheHalfInTheAppsLanguage() {
        val english = Terms.blocks(Privacy.forLanguage(sample, english = true))
        assertEquals(
            listOf(
                Terms.Block(Terms.Kind.Title, "Privacy Policy / 隐私政策"),
                Terms.Block(Terms.Kind.Paragraph, "Effective date / 生效日期：2026-11-23"),
                Terms.Block(Terms.Kind.Heading, "What we collect"),
                Terms.Block(Terms.Kind.Paragraph, "Nothing by default."),
            ),
            english,
        )
        val chinese = Terms.blocks(Privacy.forLanguage(sample, english = false))
        assertEquals(
            listOf(
                Terms.Block(Terms.Kind.Title, "Privacy Policy / 隐私政策"),
                Terms.Block(Terms.Kind.Paragraph, "Effective date / 生效日期：2026-11-23"),
                Terms.Block(Terms.Kind.Heading, "我们收集什么"),
                Terms.Block(Terms.Kind.Paragraph, "默认不收集任何数据。"),
            ),
            chinese,
        )
    }

    @Test
    fun aFileWithoutHalvesIsShownWhole() {
        assertEquals("# Policy\n\nText", Privacy.forLanguage("# Policy\n\nText", english = true))
    }

    @Test
    fun theShippedPolicyHasBothHalvesForEveryPlatform() {
        val markdown = File(PRIVACY_PATH).readText()
        val english = Privacy.forLanguage(markdown, english = true)
        val chinese = Privacy.forLanguage(markdown, english = false)
        assertTrue(Terms.blocks(english).size > 10)
        assertTrue(Terms.blocks(chinese).size > 10)
        assertFalse(english.contains("## 中文"))
        assertFalse(chinese.contains("### What we collect"))
        for (platform in listOf("Android", "iOS", "macOS", "Windows", "Linux")) {
            assertTrue("English half names $platform", english.contains("**$platform") || english.contains(platform))
        }
        // The data statement the consent screen shows rests on these promises.
        assertTrue(english.contains("Nothing by default"))
        assertTrue(chinese.contains("默认不收集任何数据"))
    }

    companion object {
        /** PRIVACY.md at the repository root, from the app module (the unit tests' working directory). */
        const val PRIVACY_PATH = "../../../PRIVACY.md"
    }
}

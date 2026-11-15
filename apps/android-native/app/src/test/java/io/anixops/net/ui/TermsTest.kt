package io.anixops.net.ui

import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TermsTest {
    private val sample = """
        # AnixOps Net 用户协议

        基准版本 2.1.0，来源：[`legal`](legal.md)。

        > 本文件不构成法律意见。

        <!-- 内部备注 -->
        ## 产品补充条款

        1. **客户端只是工具。**不提供节点，
           由其提供方负责。
        - 第二项

        ---
    """.trimIndent()

    @Test
    fun theVersionComesFromTheBaselineLine() {
        assertEquals("2.1.0", Terms.version(sample))
        assertEquals("0.0.0", Terms.version("no version here"))
    }

    @Test
    fun consentIsAskedForAgainOnANewMajorVersion() {
        assertTrue(Terms.needsConsent(null, "2.1.0"))
        assertFalse(Terms.needsConsent("2.0.3", "2.1.0"))
        assertTrue(Terms.needsConsent("1.9.0", "2.1.0"))
    }

    @Test
    fun blocksDropMarkdownMarksAndJoinWrappedLines() {
        assertEquals(
            listOf(
                Terms.Block(Terms.Kind.Title, "AnixOps Net 用户协议"),
                Terms.Block(Terms.Kind.Paragraph, "基准版本 2.1.0，来源：legal。"),
                Terms.Block(Terms.Kind.Paragraph, "本文件不构成法律意见。"),
                Terms.Block(Terms.Kind.Heading, "产品补充条款"),
                Terms.Block(Terms.Kind.Item, "客户端只是工具。不提供节点，由其提供方负责。"),
                Terms.Block(Terms.Kind.Item, "第二项"),
            ),
            Terms.blocks(sample),
        )
    }

    @Test
    fun theShippedTermsHaveAVersionAndText() {
        val markdown = File(TERMS_PATH).readText()
        assertTrue(Terms.version(markdown) != "0.0.0")
        assertTrue(Terms.blocks(markdown).size > 10)
    }

    companion object {
        /** TERMS.md at the repository root, from the app module (the unit tests' working directory). */
        const val TERMS_PATH = "../../../TERMS.md"
    }
}

package io.anixops.net.ui

/**
 * The privacy policy (PRIVACY.md at the repository root, packed into the app's assets by the build next to the user
 * agreement), shown under 设置 › 关于 and from the first-start consent. The file holds an English and a Chinese
 * half; the app shows the one in its language, under the shared title and effective date, and drops the line that
 * links the two halves. [Terms.blocks] turns the result into headings, paragraphs and list items.
 */
object Privacy {
    const val ASSET = "PRIVACY.md"

    private const val ENGLISH = "## English"
    private const val CHINESE = "## 中文"

    /** The policy in English when [english], else in Chinese; the whole file when it has no such halves. */
    fun forLanguage(markdown: String, english: Boolean): String {
        val lines = markdown.replace("\r\n", "\n").split("\n")
        val englishAt = lines.indexOfFirst { it.trim() == ENGLISH }
        val chineseAt = lines.indexOfFirst { it.trim() == CHINESE }
        if (englishAt < 0 || chineseAt < 0) return markdown
        val head = lines.subList(0, minOf(englishAt, chineseAt))
            // The "[English](#english) · [中文](#中文)" line points inside the file, which the viewer cannot follow.
            .filterNot { it.contains("](#") }
        val body = when {
            english && englishAt < chineseAt -> lines.subList(englishAt + 1, chineseAt)
            english -> lines.subList(englishAt + 1, lines.size)
            chineseAt < englishAt -> lines.subList(chineseAt + 1, englishAt)
            else -> lines.subList(chineseAt + 1, lines.size)
        }
        return (head + "" + body).joinToString("\n").trim()
    }
}

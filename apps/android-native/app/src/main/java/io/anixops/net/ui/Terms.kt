package io.anixops.net.ui

/**
 * The user agreement (TERMS.md at the repository root, packed into the app's assets by the build) as the other
 * clients read it (apps/windows-gui/ui/src/terms.tsx): its version, and its text as headings, paragraphs and list
 * items. Someone accepts it once; a new major version asks again.
 */
object Terms {
    const val ASSET = "TERMS.md"

    enum class Kind { Title, Heading, Paragraph, Item }

    data class Block(val kind: Kind, val text: String)

    private val versionLine = Regex("""基准版本\s+(\d+\.\d+\.\d+)""")
    private val comment = Regex("""<!--[\s\S]*?-->""")
    private val heading = Regex("""^(#{1,3})\s+(.*)$""")
    private val item = Regex("""^\s*(?:[-*]|\d+\.)\s+(.*)$""")
    private val link = Regex("""\[([^\]]+)]\([^)]+\)""")
    private val marks = Regex("""\*\*|`""")
    private val rule = Regex("""^-{3,}$""")
    private val quote = Regex("""^>\s?""")
    private val majorPart = Regex("""^(\d+)\.""")

    /** The version from the "基准版本 x.y.z" line, or "0.0.0" when the file has none. */
    fun version(markdown: String): String = versionLine.find(markdown)?.groupValues?.get(1) ?: "0.0.0"

    /** True when nothing was accepted yet, or what was accepted is another major version than [current]. */
    fun needsConsent(accepted: String?, current: String): Boolean {
        val major = major(accepted) ?: return true
        return major != major(current)
    }

    private fun major(version: String?): String? = version?.let { majorPart.find(it)?.groupValues?.get(1) }

    fun blocks(markdown: String): List<Block> {
        val blocks = mutableListOf<Block>()
        var open = false
        for (raw in comment.replace(markdown, "").split("\n")) {
            val line = quote.replaceFirst(raw, "")
            if (line.isBlank() || rule.matches(line.trim())) {
                open = false
                continue
            }
            val head = heading.find(line)
            if (head != null) {
                val kind = if (head.groupValues[1].length == 1) Kind.Title else Kind.Heading
                blocks += Block(kind, plain(head.groupValues[2]))
                open = false
                continue
            }
            val listItem = item.find(line)
            val last = blocks.lastOrNull()
            when {
                listItem != null -> blocks += Block(Kind.Item, plain(listItem.groupValues[1]))
                // A wrapped line continues the paragraph or list item right above it.
                open && last != null -> blocks[blocks.lastIndex] = last.copy(text = last.text + plain(line))
                else -> blocks += Block(Kind.Paragraph, plain(line))
            }
            open = true
        }
        return blocks
    }

    private fun plain(text: String): String = marks.replace(link.replace(text, "$1"), "").trim()
}

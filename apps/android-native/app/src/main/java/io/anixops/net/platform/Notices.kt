package io.anixops.net.platform

/**
 * How a notification from the client is shown: [tag] replaces the earlier notification of the same tag (an
 * unexpected disconnect, a failed subscription update), as iOS does; script notifications have none and stack.
 */
data class NoticeSlot(val tag: String?, val id: Int)

/** Where a notice of [kind] goes; [next] gives a new id for one that stacks. */
fun noticeSlot(kind: String?, next: () -> Int): NoticeSlot = when (kind) {
    "connection" -> NoticeSlot("connection", CONNECTION_NOTICE_ID)
    "subscription" -> NoticeSlot("subscription", SUBSCRIPTION_NOTICE_ID)
    else -> NoticeSlot(null, next())
}

/**
 * The link a tap on a notification opens: only http and https, which the shared client already allows alone;
 * anything else (a missing link, `javascript:`, a file) opens the app instead.
 */
fun noticeLink(url: String?): String? {
    val link = url?.trim().orEmpty()
    val scheme = link.substringBefore("://", missingDelimiterValue = "").lowercase()
    return link.takeIf { (scheme == "http" || scheme == "https") && link.length > scheme.length + 3 }
}

private const val CONNECTION_NOTICE_ID = 10
private const val SUBSCRIPTION_NOTICE_ID = 11

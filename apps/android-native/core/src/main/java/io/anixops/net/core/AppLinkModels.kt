package io.anixops.net.core

import kotlinx.serialization.Serializable

/**
 * What an `anixops://` link asks for (`view.parse_app_link`): [action] is `connect`, `disconnect`, `toggle`,
 * `mode` ([mode]), `select` ([node], maybe [group]), `select_member` ([policy], [member]) or `import` ([url]).
 * A refused link has [error] instead (`invalid`, `not_app_link`, `too_long`, `missing_action`, `unknown_action`,
 * `missing_parameter`, `unknown_mode`), with [detail] when there is something to name.
 */
@Serializable
data class AppLink(
    val action: String? = null,
    val mode: String? = null,
    val node: String? = null,
    val group: String? = null,
    val policy: String? = null,
    val member: String? = null,
    val url: String? = null,
    val error: String? = null,
    val detail: String? = null,
)

/** The policy group and member a `select_member` link names (`view.find_link_member`). */
@Serializable
data class LinkMember(val tag: String, val outbound: String)

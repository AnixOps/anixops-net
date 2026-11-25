package io.anixops.net.core

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.addJsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

/** A saved 组网 network as the backend lists it (`mesh_networks`): never its secret. */
@Serializable
data class MeshSavedNetwork(
    val name: String,
    val joined: Boolean,
    /** `forward_secrecy`, `standard` or `none`. */
    val security: String,
    val peers: Int = 0,
    val address: String? = null,
)

/**
 * What the 添加网络 form holds (docs/mesh-ui.md, 添加网络): name, secret, peers, virtual address and encryption. The
 * region is assigned by the network. The phone is an access node: no listening port, no relay, no advertised
 * subnets. The same rules as the Apple form (`MeshNetworkDraft`).
 */
data class MeshNetworkDraft(
    val name: String = "",
    val secret: String = "",
    /** One peer per line: `host:port`, `[v6]:port` or `host`, optionally with `tcp://` or `udp://` in front. */
    val peers: String = "",
    /** The IPv4 virtual address with its prefix, such as `10.144.144.5/24`; empty when the network assigns one. */
    val address: String = "",
    /** The default: forward secrecy. Off is the older mode of a network in migration, still encrypted. */
    val forwardSecrecy: Boolean = true,
) {
    sealed interface Problem {
        data object Name : Problem
        data object Secret : Problem
        data object Peers : Problem
        data class Peer(val line: String) : Problem
        data object Address : Problem
    }

    data class Peer(val server: String, val port: Int, val transport: String)

    private val peerLines: List<String>
        get() = peers.lines().map { it.trim() }.filter { it.isNotEmpty() }

    /** The first thing wrong with the form, or null when it can be saved. */
    val problem: Problem?
        get() {
            val name = name.trim()
            if (name.isEmpty() || name.codePointCount(0, name.length) > 64 || name.contains('/') ||
                name.any { it == '\n' || it == '\r' }
            ) {
                return Problem.Name
            }
            if (secret.isEmpty()) return Problem.Secret
            val lines = peerLines
            if (lines.isEmpty()) return Problem.Peers
            lines.firstOrNull { peer(it) == null }?.let { return Problem.Peer(it) }
            val address = address.trim()
            if (address.isNotEmpty() && !validAddress(address)) return Problem.Address
            return null
        }

    /** The trimmed name the network is saved under. */
    val networkName: String get() = name.trim()

    /** `save_mesh_network`'s `config`, secret included; null while [problem] is not null. */
    fun config(): JsonObject? {
        if (problem != null) return null
        return buildJsonObject {
            put("config_version", 1)
            putJsonObject("network") {
                put("name", networkName)
                put("secret", secret)
                put("kind", if (forwardSecrecy) "managed" else "legacy")
                put("secure_mode", forwardSecrecy)
            }
            putJsonObject("node") {
                put("role", "access")
                address.trim().takeIf { it.isNotEmpty() }?.let { put("address", it) }
            }
            putJsonArray("peers") {
                peerLines.mapNotNull(::peer).forEach { peer ->
                    addJsonObject {
                        put("server", peer.server)
                        put("port", peer.port)
                        put("transport", peer.transport)
                    }
                }
            }
        }
    }

    companion object {
        /** The default listening port of mesh members. */
        const val DEFAULT_PORT = 11010

        /** A peer line: `[tcp://|udp://]host[:port]`, with IPv6 hosts in brackets. */
        fun peer(line: String): Peer? {
            var rest = line.trim()
            var transport = "tcp"
            for (scheme in listOf("tcp", "udp")) {
                if (rest.lowercase().startsWith("$scheme://")) {
                    transport = scheme
                    rest = rest.substring(scheme.length + 3)
                }
            }
            var port = DEFAULT_PORT
            val host: String
            if (rest.startsWith("[")) {
                val close = rest.indexOf(']')
                if (close < 0) return null
                host = rest.substring(1, close)
                val after = rest.substring(close + 1)
                if (after.isNotEmpty()) {
                    if (!after.startsWith(":")) return null
                    port = validPort(after.substring(1)) ?: return null
                }
                if (!isIpv6(host)) return null
            } else {
                val parts = rest.split(":")
                if (parts.size > 2 || parts[0].isEmpty()) return null
                host = parts[0]
                if (parts.size == 2) port = validPort(parts[1]) ?: return null
                if (!host.all { it.isAsciiLetterOrDigit() || it == '.' || it == '-' }) return null
            }
            return Peer(host, port, transport)
        }

        /** An IPv4 address with a prefix of /8 to /30 (mesh-module.md, `node.address`). */
        fun validAddress(text: String): Boolean {
            val parts = text.split("/")
            if (parts.size != 2 || !isIpv4(parts[0])) return false
            val prefix = parts[1].toIntOrNull() ?: return false
            return parts[1].all(Char::isDigit) && prefix in 8..30
        }

        private fun validPort(text: String): Int? =
            text.takeIf { it.isNotEmpty() && it.all(Char::isDigit) && it.length <= 5 }
                ?.toInt()?.takeIf { it in 1..65535 }

        private fun Char.isAsciiLetterOrDigit() = this in 'a'..'z' || this in 'A'..'Z' || this in '0'..'9'

        internal fun isIpv4(text: String): Boolean {
            val parts = text.split(".")
            return parts.size == 4 && parts.all { part ->
                part.isNotEmpty() && part.length <= 3 && part.all(Char::isDigit) && part.toInt() <= 255 &&
                    (part.length == 1 || part[0] != '0')
            }
        }

        internal fun isIpv6(text: String): Boolean {
            if (text.isEmpty() || !text.contains(':')) return false
            val double = text.indexOf("::")
            if (double >= 0 && text.indexOf("::", double + 1) >= 0) return false
            val groups = text.split(":")
            var count = 0
            for ((index, group) in groups.withIndex()) {
                if (group.isEmpty()) continue
                if (index == groups.lastIndex && group.contains('.')) {
                    if (!isIpv4(group)) return false
                    count += 2
                    continue
                }
                if (group.length > 4 || !group.all { it.isDigit() || it.lowercaseChar() in 'a'..'f' }) return false
                count += 1
            }
            return if (double >= 0) count < 8 else count == 8 && groups.none { it.isEmpty() }
        }
    }
}

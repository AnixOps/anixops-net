package io.anixops.net.core

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** The 添加网络 form: the same rules as the Apple form (`MeshNetworksTests`). */
class MeshNetworksTest {
    private fun draft(
        name: String = "office",
        secret: String = "s3cret",
        peers: String = "203.0.113.10:11010",
        address: String = "",
    ) = MeshNetworkDraft(name, secret, peers, address)

    @Test
    fun problemsComeInFormOrder() {
        assertEquals(MeshNetworkDraft.Problem.Name, draft(name = " ").problem)
        assertEquals(MeshNetworkDraft.Problem.Name, draft(name = "a".repeat(65)).problem)
        assertEquals(MeshNetworkDraft.Problem.Name, draft(name = "a/b").problem)
        assertEquals(MeshNetworkDraft.Problem.Secret, draft(secret = "").problem)
        assertEquals(MeshNetworkDraft.Problem.Peers, draft(peers = " \n ").problem)
        assertEquals(MeshNetworkDraft.Problem.Peer("203.0.113.10:0"), draft(peers = "203.0.113.10:0").problem)
        assertEquals(MeshNetworkDraft.Problem.Peer("host:70000"), draft(peers = "host:70000").problem)
        assertEquals(MeshNetworkDraft.Problem.Peer("[2001:db8::1"), draft(peers = "[2001:db8::1").problem)
        assertEquals(MeshNetworkDraft.Problem.Peer("a:b:c"), draft(peers = "a:b:c").problem)
        assertEquals(MeshNetworkDraft.Problem.Peer("bad host"), draft(peers = "bad host").problem)
        assertEquals(MeshNetworkDraft.Problem.Address, draft(address = "10.144.144.5").problem)
        assertEquals(MeshNetworkDraft.Problem.Address, draft(address = "10.144.144.5/31").problem)
        assertEquals(MeshNetworkDraft.Problem.Address, draft(address = "10.144.144.300/24").problem)
        assertNull(draft(address = "10.144.144.5/24").problem)
        assertNull(draft(name = "办公室").problem)
    }

    @Test
    fun peerLines() {
        assertEquals(MeshNetworkDraft.Peer("mesh.example.net", 11010, "tcp"), MeshNetworkDraft.peer("mesh.example.net"))
        assertEquals(MeshNetworkDraft.Peer("203.0.113.10", 11011, "udp"), MeshNetworkDraft.peer("UDP://203.0.113.10:11011"))
        assertEquals(MeshNetworkDraft.Peer("2001:db8::1", 443, "tcp"), MeshNetworkDraft.peer("tcp://[2001:db8::1]:443"))
        assertEquals(MeshNetworkDraft.Peer("2001:db8::1", 11010, "tcp"), MeshNetworkDraft.peer("[2001:db8::1]"))
        assertEquals("::ffff:192.0.2.1", MeshNetworkDraft.peer("[::ffff:192.0.2.1]")?.server)
        assertNull(MeshNetworkDraft.peer("[not-v6]:1"))
        assertNull(MeshNetworkDraft.peer("[1:2:3:4:5:6:7:8:9]"))
        assertNull(MeshNetworkDraft.peer("[1::2::3]"))
    }

    @Test
    fun theConfigIsAnAccessNodeWithItsSecret() {
        val config = MeshNetworkDraft(
            name = " office ",
            secret = "s3cret",
            peers = "203.0.113.10\n\nudp://[2001:db8::1]:11011",
            address = "10.144.144.5/24",
            forwardSecrecy = false,
        ).config()!!
        assertEquals(1, config["config_version"]!!.jsonPrimitive.int)
        val network = config["network"]!!.jsonObject
        assertEquals("office", network["name"]!!.jsonPrimitive.content)
        assertEquals("s3cret", network["secret"]!!.jsonPrimitive.content)
        assertEquals("legacy", network["kind"]!!.jsonPrimitive.content)
        assertEquals(false, network["secure_mode"]!!.jsonPrimitive.boolean)
        val node = config["node"]!!.jsonObject
        assertEquals("access", node["role"]!!.jsonPrimitive.content)
        assertEquals("10.144.144.5/24", node["address"]!!.jsonPrimitive.content)
        val peers = config["peers"]!!.jsonArray
        assertEquals(2, peers.size)
        assertEquals(
            JsonObject(
                mapOf(
                    "server" to JsonPrimitive("2001:db8::1"),
                    "port" to JsonPrimitive(11011),
                    "transport" to JsonPrimitive("udp"),
                ),
            ),
            peers[1],
        )

        val managed = draft().config()!!
        assertEquals("managed", managed["network"]!!.jsonObject["kind"]!!.jsonPrimitive.content)
        assertEquals(true, managed["network"]!!.jsonObject["secure_mode"]!!.jsonPrimitive.boolean)
        assertNull(managed["node"]!!.jsonObject["address"])
        assertNull(draft(secret = "").config())
    }
}

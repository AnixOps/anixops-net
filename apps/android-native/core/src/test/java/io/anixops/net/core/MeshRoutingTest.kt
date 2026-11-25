package io.anixops.net.core

import kotlinx.serialization.json.Json
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

/** Custom paths: the snapshot by member id, `mesh_set_paths` by virtual address (the desktop's mesh.test.ts too). */
class MeshRoutingTest {
    private val status = Json.parseToJsonElement(
        """
        {"self":{"id":"me","address":"10.144.0.2/16"},
         "members":[{"id":"a","address":"10.144.0.1/16"},{"id":"b","address":"10.144.1.1/16"},{"id":"c"}],
         "paths":[{"id":"p1","target":{"subnet":"172.20.0.0/16"},"via":["a","b"],"state":"active"},
                  {"id":"p2","target":{"peer":"b"},"via":["a"],"state":"fallback"}]}
        """,
    )

    private fun json(text: String) = Json.parseToJsonElement(text)

    @Test
    fun readsTheSnapshotsPaths() {
        assertEquals(
            listOf(
                "p1" to MeshRouting.Draft(false, "172.20.0.0/16", listOf("a", "b")),
                "p2" to MeshRouting.Draft(true, "b", listOf("a")),
            ),
            MeshRouting.current(status),
        )
    }

    @Test
    fun sendsTheWholeListByAddress() {
        assertEquals(json("""[{"to_peer":"10.144.1.1","via":["10.144.0.1"]}]"""), MeshRouting.paths(status, remove = "p1"))
        assertEquals(
            json(
                """[{"to_subnet":"172.20.0.0/16","via":["10.144.0.1","10.144.1.1"]},
                    {"to_peer":"10.144.1.1","via":["10.144.0.1"]},
                    {"to_subnet":"192.168.9.0/24","via":["10.144.1.1"]}]""",
            ),
            MeshRouting.paths(status, add = MeshRouting.Draft(false, "192.168.9.0/24", listOf("b"))),
        )
    }

    @Test
    fun replacesTheSameTargetAndRefusesWhatCannotBeSent() {
        assertEquals(
            json(
                """[{"to_subnet":"172.20.0.0/16","via":["10.144.0.1","10.144.1.1"]},
                    {"to_peer":"10.144.1.1","via":["10.144.0.2"]}]""",
            ),
            MeshRouting.paths(status, add = MeshRouting.Draft(true, "b", listOf("me"))),
        )
        assertNull(MeshRouting.paths(status, add = MeshRouting.Draft(true, "c", listOf("a"))))
        assertNull(MeshRouting.paths(status, add = MeshRouting.Draft(false, "10.9.0.0/16", emptyList())))
        assertNull(MeshRouting.paths(status, add = MeshRouting.Draft(false, "10.9.0.0/16", List(7) { "a" })))
    }
}

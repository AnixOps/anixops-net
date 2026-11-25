package io.anixops.net.ui

import io.anixops.net.core.MeshAdvertiser
import io.anixops.net.core.MeshContains
import io.anixops.net.core.MeshCustomPath
import io.anixops.net.core.MeshEntry
import io.anixops.net.core.MeshGroup
import io.anixops.net.core.MeshGroupLink
import io.anixops.net.core.MeshLink
import io.anixops.net.core.MeshMemberRow
import io.anixops.net.core.MeshMembers
import io.anixops.net.core.MeshNetwork
import io.anixops.net.core.MeshPage
import io.anixops.net.core.MeshPaths
import io.anixops.net.core.MeshRegion
import io.anixops.net.core.MeshRegionBasis
import io.anixops.net.core.MeshSavedNetwork
import io.anixops.net.core.MeshSelf
import io.anixops.net.core.MeshStatusHead
import io.anixops.net.core.MeshSubnetRow
import io.anixops.net.core.MeshSubnets
import kotlinx.serialization.json.Json

/**
 * The 组网 page of the screenshots: a joined network with members in two regions, the same made-up network as the
 * iPhone demo (MeshDemo in apps/ios), as the client-view commands answer it.
 */
internal object MeshFixtures {
    private val status = Json.parseToJsonElement(
        """{"statusVersion":1,"available":true,"members":[""" +
            """{"id":"m-1","name":"fra-border"},{"id":"m-2","name":"ams-gw"},""" +
            """{"id":"m-3","name":"lon-host"},{"id":"m-4","name":"tyo-border"}]}""",
    )

    private val head = MeshStatusHead(
        network = MeshNetwork(id = "office", name = "office", state = "joined", security = "forward_secrecy"),
        own = MeshSelf(
            id = "m-self",
            name = "par-phone",
            address = "10.144.144.5/24",
            domain = "par-phone.eu.mesh.internal",
            region = "eu",
            regionBasis = MeshRegionBasis(border = "fra-border", latencyMs = 12),
        ),
        regions = listOf(MeshRegion("eu", "欧洲", local = true), MeshRegion("ap", "亚太")),
    )

    private val members = MeshMembers(
        groupBy = "region",
        total = 5,
        matched = 5,
        groups = listOf(
            MeshGroup(
                key = "eu",
                count = 4,
                matched = 4,
                rows = listOf(
                    MeshMemberRow(
                        id = "m-self",
                        name = "par-phone",
                        isSelf = true,
                        role = "client",
                        region = "eu",
                        address = "10.144.144.5",
                        domain = "par-phone.eu.mesh.internal",
                        regionDomain = "eu.mesh.internal",
                        connection = "self",
                    ),
                    MeshMemberRow(
                        id = "m-2",
                        name = "ams-gw",
                        role = "gateway",
                        region = "eu",
                        address = "10.144.144.2",
                        domain = "ams-gw.eu.mesh.internal",
                        regionDomain = "eu.mesh.internal",
                        connection = "relay",
                        hops = 1,
                        transport = "udp",
                        latencyMs = 24,
                        tone = "fast",
                        contains = MeshContains("subnets", 1),
                    ),
                    MeshMemberRow(
                        id = "m-1",
                        name = "fra-border",
                        role = "border",
                        region = "eu",
                        address = "10.144.144.1",
                        domain = "fra-border.eu.mesh.internal",
                        regionDomain = "eu.mesh.internal",
                        connection = "direct",
                        transport = "udp",
                        latencyMs = 11,
                        tone = "fast",
                    ),
                    MeshMemberRow(
                        id = "m-3",
                        name = "lon-host",
                        role = "host",
                        region = "eu",
                        address = "10.144.144.3",
                        domain = "lon-host.eu.mesh.internal",
                        regionDomain = "eu.mesh.internal",
                        connection = "direct",
                        transport = "tcp",
                        latencyMs = 9,
                        tone = "fast",
                        contains = MeshContains("vms", 2),
                    ),
                ),
            ),
            MeshGroup(
                key = "ap",
                count = 1,
                matched = 1,
                open = false,
                link = MeshGroupLink(latencyMs = 214),
                rows = listOf(
                    MeshMemberRow(
                        id = "m-4",
                        name = "tyo-border",
                        role = "border",
                        region = "ap",
                        address = "10.144.146.1",
                        domain = "tyo-border.ap.mesh.internal",
                        regionDomain = "ap.mesh.internal",
                        connection = "cross_region",
                        hops = 1,
                        transport = "udp",
                        latencyMs = 228,
                        tone = "medium",
                        contains = MeshContains("subnets", 1),
                    ),
                ),
            ),
        ),
    )

    private val subnets = MeshSubnets(
        rows = listOf(
            MeshSubnetRow(
                cidr = "192.168.20.0/24",
                region = "eu",
                regionName = "欧洲",
                advertiser = "ams-gw",
                advertisers = listOf(MeshAdvertiser("m-2", "ams-gw", inUse = true)),
                path = listOf("fra-border", "ams-gw"),
                latencyMs = 26,
                tone = "fast",
            ),
            MeshSubnetRow(
                cidr = "192.168.30.0/24",
                region = "ap",
                regionName = "亚太",
                advertiser = "tyo-border",
                advertisers = listOf(MeshAdvertiser("m-4", "tyo-border", inUse = true)),
                path = listOf("fra-border", "tyo-border"),
                latencyMs = 231,
                tone = "medium",
            ),
        ),
    )

    private val paths = MeshPaths(
        policy = "latency",
        links = listOf(
            MeshLink(
                from = "eu",
                fromName = "欧洲",
                to = "ap",
                toName = "亚太",
                borders = listOf("fra-border", "tyo-border"),
                latencyMs = 214,
                tone = "medium",
                loss = 0.0,
            ),
        ),
        custom = listOf(
            MeshCustomPath(
                id = "p-1",
                targetKind = "subnet",
                target = "192.168.30.0/24",
                targetLabel = "192.168.30.0/24",
                advertiser = "tyo-border",
                via = listOf("fra-border"),
            ),
        ),
    )

    val page = MeshPage(
        status = status,
        head = head,
        entry = MeshEntry(available = true, network = "office", state = "joined", security = "forward_secrecy", online = 4, total = 4),
        members = members,
        subnets = subnets,
        paths = paths,
        notices = emptyList(),
    )

    val state = MeshState(
        page = page,
        saved = listOf(MeshSavedNetwork(name = "office", joined = true, security = "forward_secrecy", peers = 4, address = "10.144.144.5/24")),
    )
}

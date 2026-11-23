package io.anixops.net.platform

import org.junit.Assert.assertEquals
import org.junit.Test

/** Android 8 and 9 write to the folder themselves and must not overwrite an earlier file (finding android-04). */
class DownloadNamesTest {
    @Test
    fun aTakenNameGetsACopyNumber() {
        val taken = mutableSetOf("report.log")
        assertEquals("other.log", freeName("other.log") { it in taken })
        assertEquals("report (1).log", freeName("report.log") { it in taken })
        taken += "report (1).log"
        assertEquals("report (2).log", freeName("report.log") { it in taken })
        assertEquals("README (1)", freeName("README") { it == "README" })
        assertEquals(".hidden (1)", freeName(".hidden") { it == ".hidden" })
    }
}

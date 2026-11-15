package io.anixops.net.ui

import com.github.takahirom.roborazzi.captureRoboImage
import io.anixops.net.core.RuntimeSnapshot
import io.anixops.net.core.StatusFact
import io.anixops.net.ui.theme.AnixTheme
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The screenshot baselines (src/test/screenshots): each page in light and dark, Simplified Chinese and English,
 * phone and tablet. CI records them again and fails when they differ from the committed ones; a deliberate UI
 * change commits the new images with it (apps/android-native/README.md).
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [35])
class ScreenshotTest {
    @Test
    fun homeConnectedPhoneLightZh() = capture("home-connected-phone-light-zh", PHONE, "zh-rCN", dark = false)

    @Test
    fun homeConnectedPhoneDarkZh() = capture("home-connected-phone-dark-zh", PHONE, "zh-rCN", dark = true)

    @Test
    fun homeConnectedPhoneLightEn() = capture("home-connected-phone-light-en", PHONE, "en", dark = false)

    @Test
    fun homeFailedPhoneLightZh() = capture(
        "home-failed-phone-light-zh",
        PHONE,
        "zh-rCN",
        dark = false,
        state = UiState(
            snapshot = connected.copy(
                connection = "Connection failed",
                lastError = "节点 tokyo-01 握手超时（10 秒）",
            ),
        ),
    )

    @Test
    fun homeEmptyPhoneDarkEn() = capture(
        "home-empty-phone-dark-en",
        PHONE,
        "en",
        dark = true,
        state = UiState(snapshot = connected.copy(connection = "Not connected", selectedNode = null, subscription = null)),
    )

    @Test
    fun homeConnectedTabletLightZh() = capture("home-connected-tablet-light-zh", TABLET, "zh-rCN", dark = false)

    @Test
    fun homeConnectedTabletDarkEn() = capture("home-connected-tablet-dark-en", TABLET, "en", dark = true)

    @Test
    fun settingsPhoneLightZh() =
        capture("settings-phone-light-zh", PHONE, "zh-rCN", dark = false, destination = Destination.Settings)

    @Test
    fun settingsPhoneDarkEn() =
        capture("settings-phone-dark-en", PHONE, "en", dark = true, destination = Destination.Settings)

    @Test
    fun debugRequestsPhoneLightZh() =
        capture("debug-requests-phone-light-zh", PHONE, "zh-rCN", dark = false, mode = AppMode.Debug)

    private fun capture(
        name: String,
        device: String,
        locale: String,
        dark: Boolean,
        state: UiState = UiState(snapshot = connected),
        mode: AppMode = AppMode.Proxy,
        destination: Destination? = null,
    ) {
        RuntimeEnvironment.setQualifiers("$locale-$device-${if (dark) "night" else "notnight"}-xhdpi")
        captureRoboImage("src/test/screenshots/$name.png") {
            AnixTheme {
                AnixScaffold(
                    state = state,
                    mode = mode,
                    version = "0.3.0",
                    onConnect = {},
                    onDisconnect = {},
                    onModeChange = {},
                    initial = destination,
                )
            }
        }
    }

    private companion object {
        const val PHONE = "w411dp-h891dp"
        const val TABLET = "w1280dp-h800dp"

        val connected = RuntimeSnapshot(
            platform = "android",
            connection = "Connected",
            core = StatusFact(label = "Running", tone = "success"),
            selectedNode = "tokyo-01",
            subscription = "https://panel.example.com/api/v1/client/subscribe?token=demo",
            routingMode = "rule",
            dnsPreset = "system",
        )
    }
}

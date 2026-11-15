# AnixOps Net for Android (native)

The Android client rewritten in Kotlin, Jetpack Compose and Material 3, over the same Rust client as the desktop
(`crates/client-core`). Plan, decisions and migration checklist: [docs/architecture/android-native-compose.md](../../docs/architecture/android-native-compose.md).

It replaced the Tauri app (`apps/android`, removed at the cutover) under the same id, `io.anixops.net.android`: with
the same signing key it installs over that app and keeps its data directory (`files/anixops-net`), so subscriptions,
nodes, plugins and settings stay.

## Layout

| Path | What |
|---|---|
| `rust/` | `libanixops_net.so`: JNI entry points (`NativeCore.nativeInit` / `nativeInvoke` / `nativeShutdown`) over `client_core::client::Client`, and the `Platform` that calls back into Kotlin |
| `core/` | Kotlin side of the bridge: `NativeCore`, the `{"ok"}` / `{"error"}` envelope, typed commands (`Core`) and models. No UI; JVM tests read the shared fixtures in `crates/client-core/tests/fixtures` |
| `app/` | Compose UI (`ui/`), brand tokens (`ui/theme/BrandTokens.kt`, generated), and Android services (`platform/`: VpnService, Downloads, Keystore, network, notifications) |

Commands keep the shared interface's names and JSON shapes (`apps/windows-gui/ui/src/bridge.ts`), so a page moved
from the web interface calls the same command with the same arguments.

## Build and checks

Everything runs in CI (job `android-native` in `.github/workflows/ci.yml`, through `.github/actions/android-apk`,
which `release.yml` uses for the published APK); nothing is built locally in this repo. The job:

1. builds `rust/` for arm64 with `cargo ndk` into `app/src/main/jniLibs` (not committed);
2. runs `gradle lint testDebugUnitTest :app:recordRoborazziDebug :app:assembleRelease` (lint warnings fail);
3. fails when the recorded screenshots differ from `app/src/test/screenshots` and uploads the new ones;
4. signs the APK with `.github/actions/sign-apk` (the release keystore when the secrets are set, otherwise a
   throwaway key) and uploads it as `anixops-net-android-<sha>`.

### Screenshot baselines

`app/src/test/java/io/anixops/net/ui/ScreenshotTest.kt` draws each page with Robolectric (no emulator): light and
dark, Simplified Chinese and English, phone (411×891 dp) and tablet (1280×800 dp). A UI change commits its new
images in the same PR: take them from the `android-native-screenshots-<sha>` artifact of the failing run, check them,
and commit them under `app/src/test/screenshots`.

### Design tokens

`app/src/main/java/io/anixops/net/ui/theme/BrandTokens.kt` comes from `apps/ios/Design/tokens.json` and AnixOps
Design. Change the tokens there and run `python3 .github/scripts/design_tokens.py`; CI checks it is up to date.

# iOS Source Tree Governance Placeholder

Current status: `swift-source-gate-active`.

This directory is the iOS application and Network Extension source tree root. The Swift package, the XcodeGen spec and
the `NetworkCorePacketTunnel` extension target are active. Signing, archive/export and upload material remain blocked until
their own gates pass.

## Ownership

- The iOS Swift package is owned by `apps/ios`. Its manifest is `apps/ios/Package.swift` and no other `Package.swift` may exist.
- Package ownership is governed by
  `docs/architecture/ios-package-swift-source-ownership-activation-preflight-contract.md`.
- Manifest-only activation is governed by
  `docs/architecture/ios-package-swift-manifest-only-activation-validation-contract.md`.
- Package targets: `NetworkCoreBridge` and `NetworkCoreApp` (library product `NetworkCoreIOS`), with `NetworkCoreBridgeTests`
  as the test target.
- `NetworkCorePacketTunnel` is the only approved Network Extension target name. It is declared in `project.yml` as an
  app extension, not in `Package.swift`. It links the embedded Rust runtime (`crates/engine-embedded`) as
  `Generated/NetworkCoreRuntime.xcframework`, which GitHub Actions builds with
  `.github/scripts/build-ios-runtime-xcframework.sh` and never commits. The extension imports the C ABI as the Clang
  module `NetworkCoreRuntimeC` and fails closed when the runtime handshake or the tunnel profile is invalid. The containing
  app links the same library but never starts a tunnel with it: it uses the node importer (crates/node-import, Clang
  module `NetworkCoreImportC`, shipped in the same XCFramework) to turn pasted text into nodes, and
  `networkcore_ios_measure_delay` for the delay test, an HTTP request to `http://www.gstatic.com/generate_204` through
  each node's own protocol.
- Tunnel profile: the app imports one node (`TunnelNodeImport`). Shadowsocks-2022 `ss://`, VLESS REALITY `vless://`
  links and single-peer WireGuard `wg-quick` configs are read in Swift; any other share link the engine runs (Trojan,
  VMess, VLESS over TLS/WebSocket/gRPC, Shadowsocks AEAD, Hysteria2, TUIC) and subscription texts (base64 link lists,
  Clash YAML, JSON outbound configs, Surge/Loon/Quantumult X proxy lines) go through the Rust importer, and the first runnable
  node becomes a native outbound whose settings are the Keychain secret. Node secrets (the Shadowsocks password, the WireGuard private and preshared keys) go into the
  Keychain as one `NodeSecret` JSON value (`NodeSecretStore`, `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`) and
  the profile keeps only its persistent reference in `NETunnelProviderProtocol.passwordReference`;
  `providerConfiguration` holds the runtime config without secrets and a `TunnelInterface` (addresses, routes, DNS,
  MTU) that the extension turns into `NEPacketTunnelNetworkSettings`. Shadowsocks uses the fixed full-tunnel
  interface; WireGuard uses the config's `Address`, `AllowedIPs`, `DNS` and `MTU`. Both targets list `$(AppIdentifierPrefix)networkcore.shared` first in `keychain-access-groups`, so the
  extension reads the item the app saved. The default team-prefixed profiles already allow this group.
- UI: the app follows AnixOps Design (github.com/AnixOps/anixops-design), the brand and token system shared by every
  AnixOps product (five tabs: 首页, 代理, 规则, 扩展, 设置). The design repository is vendored unmodified into
  `apps/windows-gui/ui/src/design/anixops/` by `python3 .github/scripts/sync_design.py --source <clone> --ref <tag>`
  (add `--icons` to refresh the app icons from it); `apps/ios/Design/tokens.json` only maps it onto the client's names.
  `python3 .github/scripts/design_tokens.py` generates `Sources/NetworkCoreAppHost/BrandTokens.swift` and the desktop
  `apps/windows-gui/ui/src/design/tokens.css` from both. CI fails when the vendored copy differs from its manifest or a
  generated file is stale; never edit them by hand.
- Widgets: `Sources/NetworkCoreWidget` holds the Home Screen / Lock Screen status widget and the iOS 18 Control
  Center switch; `Sources/NetworkCoreShared` (status reader and the connect intent) is compiled into the app and the
  widget. The TestFlight upload ships the widget only once its profile is configured (see `docs/manual-intervention.md`).
- Languages: Chinese (zh-Hans) is the source language and English comes from
  `Sources/NetworkCoreApp/Resources/Localizable.xcstrings`; the app follows the iPhone language. SwiftUI text literals are
  keys as written; strings built in code use `String(localized:)`. Add every new user-visible string to the catalog.
- Swift sources may only live under `apps/ios/Sources` or `apps/ios/Tests`.
- Future Xcode project or workspace files must reference this source tree and must not become the only source of truth.

## Current Boundary

Current boundary: `Package.swift`, the Swift package sources under `Sources/` and `Tests/`, the XcodeGen spec `project.yml`,
the `NetworkCorePacketTunnel` extension target, the minimal entitlements under `Entitlements/` and the `PrivacyInfo.xcprivacy`
manifests under `Sources/*/Resources/` are enabled. The generated `.xcodeproj` is never committed.

These files and directories must stay absent until their own activation gates are complete:

- `*.swift` outside `apps/ios/Sources` and `apps/ios/Tests`
- `*.xcodeproj`
- `*.xcworkspace`
- `PrivacyInfo.xcprivacy` outside `apps/ios/Sources/*/Resources`
- `*.entitlements` outside `apps/ios/Entitlements`
- `ExportOptions.plist`
- `.ipa`, `.xcarchive`, `.xcresult` or dSYM bundles
- Provisioning Profile, certificate, private key, temporary keychain or App Store Connect API material

`swift build` and `swift test` run only in GitHub Actions on `macos-26`, never locally.

## CI Hook

GitHub Actions statically checks this README and runs the `macos-26` manifest validation hook,
`.github/scripts/verify-ios-package-manifest.sh`. Any Swift/Xcode source scan, Swift build, Swift test, Xcode project validation,
signing, archive/export or upload validation must run only in GitHub Actions on `macos-26` or Apple official platforms.

`ios-upload-workflow-status=enabled` (2026-10-22) unlocks only the manual internal TestFlight workflow,
`.github/workflows/ios-testflight.yml`. External TestFlight, App Store submission and release assets stay blocked.

## App icon

`Design/AppIcon.svg` is the source of the app icon. The asset catalog uses one
1024×1024 sRGB PNG without an alpha channel
(`Sources/NetworkCoreApp/Resources/Assets.xcassets/AppIcon.appiconset/AppIcon-1024.png`);
iOS applies the corner mask. Re-export the PNG at 1024×1024 and drop the alpha
channel whenever the SVG changes.

## HTTPS decryption certificate

The "扩展" tab hosts the MITM root certificate wizard (roadmap M4). The app generates a per-device P-256 root
with the Security framework (`MitmCertificateAuthority`, no Rust in the app), stores the certificate and its private
key in the shared Keychain group (`MitmCertificateStore`, service `networkcore.mitm-ca`) for the packet tunnel, and
serves a `com.apple.security.root` configuration profile on `127.0.0.1` for Safari to hand to Settings.
Trust is detected by asking `SecTrust` to accept a probe leaf with the system store: `notGenerated`, `notTrusted`
(profile missing or full trust off; iOS does not tell these apart) or `trusted`.

On macOS the "扩展" page (`MacExtensionsPage`) shows the same plugin and script section. There is no profile step:
"信任证书…" adds the root to the login keychain and sets user trust settings to always trust
(`SecTrustSettingsSetTrustSettings`, macOS asks for the login password), with "在“钥匙串访问”中打开" as the manual
fallback. The root and key stay in the data protection keychain under the shared access group, the only macOS
keychain the tunnel extension can read. Firefox keeps its own certificate list.

## Plugins

The same tab imports Loon plugins from a file, the clipboard or an `https` address the user types (D4: remote
import only on the user's request; downloads are capped and a redirect to plain HTTP is refused).
`MitmPluginLibrary` keeps them in the app's Application Support folder with a master "HTTPS 解密" switch, which
turns on only once the root is trusted. The enabled plugin texts (up to 8 MiB each and 8 MiB together) go into the tunnel profile under
`TunnelProfileKeys.mitmPlugins`, zlib-compressed like the scripts (`ProfileCompression`); the extension adds the root from the Keychain and passes both to the runtime as
the config's `mitm` section, where `mitm_policy::loon_plugin` translates and merges them (see
`crates/engine-embedded/README.md`). The root key never enters the profile. If the root is missing or the runtime
refuses the plugins, the tunnel starts without decryption and says why in developer-mode diagnostics. A change restarts
a running tunnel, since the extension reads them only at start. Plugin `[Rule]` lines apply ahead of the user's rules, and plain HTTP always goes through plugin rewrites and scripts.

Scripts never run on import. The "脚本" section lists each `script-path` of the enabled plugins (relative paths
resolve against a remote plugin's address; anything that is not `https` cannot be allowed). Allowing one shows its
address, downloads it (at most 4 MiB) and stores that copy with its SHA-256 in `MitmScriptLibrary`; a changed file at
the same address needs a new agreement, and a swipe revokes it. The allowed sources of the enabled plugins (at most
8 MiB) go into the profile under `TunnelProfileKeys.mitmScripts`. The extension writes them to its Caches folder by
digest and passes them as `script_assets`, with `$persistentStore` in its Application Support folder; scripts run in
QuickJS inside the tunnel without `$httpClient`. If the runtime refuses the scripts, the tunnel keeps decryption
without them.

## URL scheme

The iOS and macOS apps register `anixops://` (`CFBundleURLTypes` in `project.yml`). `AppURLCommand` in
NetworkCoreBridge parses a link and `TunnelController.handle(url:)` runs it:

| Link | Action |
| --- | --- |
| `anixops://connect` | Connect with the selected node (aliases `start`, `on`) |
| `anixops://disconnect` | Disconnect (aliases `stop`, `off`) |
| `anixops://toggle` | Connect or disconnect |
| `anixops://mode?set=rule\|global\|direct` | Switch the proxy mode |
| `anixops://select?node=<name>[&group=<subscription>]` | Use a node of a subscription group (the active one by default); `group` matches an id or a name |
| `anixops://select?policy=<policy group>&node=<member>` | Pick a member of a `select` policy group in the subscription's rules |
| `anixops://import?url=<link>` | Open the import sheet with that subscription or node link (alias `install-config`) |

Parameters are percent-encoded. `anixops:///connect` and `anixops://x-callback-url/connect` work too. An import
link never saves anything by itself: the sheet shows the preview and the user confirms. A link the app cannot run
shows a notice with the action or parameter it could not use. Links longer than 8 KiB are refused.

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
  node becomes a native outbound whose settings are the Keychain secret. An importer node takes a Swift case
  (Shadowsocks, VLESS REALITY over TCP) only when it has exactly that case's fields; one with more (a `via`, a
  transport, multiplexing) and every importer WireGuard node stay native, so nothing the importer wrote is dropped and
  the profile copy of such a node keeps only `type`, `server` and `port` (`EmbeddedRuntimeConfig.Outbound`). Node secrets (the Shadowsocks password, the WireGuard private and preshared keys) go into the
  Keychain as one `NodeSecret` JSON value (`NodeSecretStore`, `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`) and
  the profile keeps only its persistent reference in `NETunnelProviderProtocol.passwordReference`;
  `providerConfiguration` holds the runtime config without secrets and a `TunnelInterface` (addresses, routes, DNS,
  MTU) that the extension turns into `NEPacketTunnelNetworkSettings`. Shadowsocks and every importer node use the fixed
  full-tunnel interface; a pasted WireGuard config uses its `Address`, `AllowedIPs`, `DNS` and `MTU` and runs as the
  whole-device tunnel. A WireGuard node from a subscription or `wireguard://` link runs in the engine's userspace stack
  with its own `local_address`: as a node of the routing section, and when it runs alone (no node list, or the engine
  refused the rules) as the only node of one (`EmbeddedRuntimeConfig.selectedNodeAlone()`). Both targets list `$(AppIdentifierPrefix)networkcore.shared` first in `keychain-access-groups`, so the
  extension reads the item the app saved. The default team-prefixed profiles already allow this group.
- Profile writes: the widget, the Control Center switch, the Live Activity, Shortcuts and Settings change the same VPN
  configuration from other processes. Every change the app makes to it and to the node keys runs one at a time in
  `TunnelController`'s `SerialAsyncQueue`, on a copy read right before (`ConfigurationWrite`): a write sets only the
  fields it owns, so a new node keeps the saved mode and On Demand switch, and a save refused as stale is replayed once
  on a fresh copy. Each saved profile names its own key version (`TunnelProfileKeys.nodeKeyVersion`); after a save the
  app removes the other versioned keys except those of saves still on their way, and none while a tunnel is starting.
  The app reads the configuration again on `NEVPNConfigurationChange` and when it comes to the front, and the active
  subscription's rules switch follows a mode Shortcuts or Control Center saved (`SubscriptionGroups.followingSavedMode`).
  Tapping a node whose save is pending waits for that save. At launch a profile whose key is missing (left by
  0.3.0-beta.11) is saved again for the selected node. On macOS every saved write and node library change is also
  handed to a running System Extension (`TunnelAppMessage.startSecrets`); a start the system made on a copy older
  than the profile (an older node key version, HTTPS decryption chunks that do not match the profile's digest
  after a plugin or debug mapping change, or another MITM root than the profile's fingerprint) fails with `start secrets are older than the profile` and the app, when it
  runs or next becomes active, starts the tunnel once more with options for that profile (node key version, payload
  reference and root fingerprint; remembered across launches, so it never loops). Known limit: a group member picked while the
  tunnel is down keeps neither a new key version nor a new payload, so a system-initiated start runs the previous
  selections until the app connects or hands the copy over again.
- Subscription updates and the running tunnel: the extension reads the node and the node library only at start, so
  an import, a manual update or an automatic update that changes what the running tunnel would run (another active
  group, another node, changed credentials, nodes or rules; `SubscriptionRefresh.changesRunningTunnel`) reconnects
  it, after the library is stored where the start reads it. An update that only brings a new update time leaves the
  tunnel alone. The iOS background refresh (`BGAppRefreshTask`, which can launch the app without a window, so it
  reads the saved data first) never stops the tunnel, as the app may be suspended before it could start it again: it
  marks the connection, which reconnects the next time the app comes to the front unless it came up again since.
  Without a saved VPN configuration an automatic update keeps the nodes and the first connect saves the profile.
  Only `https://` subscription links are accepted: App Transport Security refuses plain HTTP downloads, so an
  `http://` link is refused at the preview with that reason (other clients accept it).
- Local rule edits on a running tunnel: the extension builds the new rules on the node library its start read
  (`ReloadBase.nodeLibrary`), so a policy group pick made in place or a background update saved since does not turn
  the in-place swap into a restart.
- Live Activity: its disconnect button runs `StopTunnelActivityIntent`, a `LiveActivityIntent` that the system runs in
  the app's process; it stops the tunnel and ends the activity at once. A stop the suspended app does not see
  (Settings, Control Center, a crash) ends the activity at the next background refresh or when the app opens.
- macOS: the command-line service, the metrics endpoint, the relay of script notifications and the automatic
  subscription updates and update checks run for the life of the app (`MacAppState.startApp`, `AppLifetimeTasks`),
  not of the main window, so they go on in the menu bar after the window is closed. Quitting removes the CLI socket.
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

`Design/AppIcon.svg` is the source of the app icon, copied from AnixOps Design
`brand/assets/products/net/app-icon.svg` (the AnixOps Net product icon, brand.md 2.6). The asset catalog uses one
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
keychain an app-extension tunnel can read. The Developer ID build's System Extension runs as root and reads no
Keychain at all: the app hands it the root, node key, node library, HTTPS decryption payload and license at start
(`TunnelStartSecrets`), and again whenever one of them or the saved profile changes while the tunnel runs
(`TunnelAppMessage.startSecrets`), so the copy the extension keeps for starts without options (On Demand, login,
System Settings) follows the saved profile. Firefox keeps its own certificate list.

Generating or importing a new root (on iOS and macOS alike) calls `TunnelController.rootCertificateChanged()`: the
profile records the new root's SHA-256 fingerprint (`TunnelProfileKeys.mitmRootFingerprint`), a running macOS
extension is handed the new values (`TunnelAppMessage.startSecrets`, waiting for its answer) and a running tunnel
that decrypts anything reconnects, since the extension reads the root only at start. A macOS start the system makes
without the app's values on a kept copy whose root does not match the profile's fingerprint fails like an older
payload (`start secrets are older than the profile`, `TunnelStartSecrets.mitmRootFailureReason`) and the app starts
the tunnel once more with options; a profile saved before the fingerprint existed checks nothing. While decryption
is on the extension reports the root it issues from (`HttpsDecryptionState.rootFingerprint`), and the request page
asks to reconnect when it differs from the Keychain's. After the tunnel moved, the Mac takes back its trust in the
replaced root (`MitmRootRevocation`): the user trust setting first (`SecTrustSettingsRemoveTrustSettings`, login
password), then the login keychain item, then a trust check of the old root. Deleting only the item, as builds
before 1.0.0 did, left the old root trusted; see docs/manual-intervention.md for removing such leftovers by hand.

## Plugins

The same tab imports Loon plugins from a file, the clipboard or an `https` address the user types (D4: remote
import only on the user's request; downloads are capped and a redirect to plain HTTP is refused).
`MitmPluginLibrary` keeps them in the app's Application Support folder with a master "HTTPS 解密" switch, which
turns on only once the root is trusted. The enabled plugin texts go into the tunnel profile (the app accepts up to
8 MiB each and 8 MiB together, the debug mappings' generated plugin included, but with the kernel this version pins
the whole engine config, plugins included, must fit in 64 KiB, so in practice decryption works only while the enabled
plugins and mappings stay well under 64 KiB; see "Engine budgets") under
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

## Engine budgets

The kernel the app is built with takes at most 64 KiB of JSON config per start (`MAX_CONFIG_LEN` of
`networkcore_ios_runtime_init`), 8 MiB of decryption plugin text in total (`MAX_POLICY_BYTES`), 64 routed outbounds
and 32 rule sets. `EngineBudget` mirrors the first two and the extension measures each config before handing it over,
leaving out the most expendable extra with a reason instead of letting the engine refuse it: allowed scripts, then
HTTPS decryption (`HttpsDecryptionState.Reason.tooLarge`, shown on the requests page), then the subscription's and the
user's rules, when the tunnel runs the selected node alone (`RoutingState`, `TunnelAppMessage.routing`, shown on the
rules page). The config the engine actually runs is the one a later local-rules change is compared with, so a tunnel
that fell back restarts instead of swapping rule lines into an engine without rules. A config still over the budget
without any extra (very large DNS rules or redirects) fails to start with the size in its reason.

A node list without subscription rules routes with only the selected node and the nodes it connects through
(`via`), however long the list. The routing section's rule sets, the DNS rules' included, are cut to 32 in a fixed
order (the user's rule lists, the user's GEOIP / IP-ASN data lists, lists only the DNS rules use, the catalog's lists,
then the subscription's own), and the rules page names what was left out. The app checks the 8 MiB decryption budget
when a plugin is added or enabled and when debug mappings grow (the mappings' plugin, with its Base64 bodies, at most
4 MiB); turning things off or removing them is always allowed.

**The 64 KiB config limit is the one that applies on iOS / macOS for now.** The plugin texts, the debug mappings'
Base64 bodies and the routing section's rules all travel inline in the one JSON config, so a single common ad-block
plugin (roughly 68–450 KB), one captured Map Local body over about 46 KB, or a subscription with more than about
1,500 inline rules is over it, although each is far within the 8 MiB and 4 MiB budgets above. The plugins and mappings
pages warn as soon as the enabled plugins and the mappings' plugin cannot fit
(`EngineBudget.decryptionExceedsConfig`, measured as the JSON strings the config carries); a set just under 64 KiB can
still miss once the node, the rules and the root are added, which the requests page reports after connecting. Raising
the limit needs a kernel change (raise `MAX_CONFIG_LEN` in engine-embedded `ffi.rs`, or pass the decryption and
routing sections as files like the scripts); it is tracked in `docs/manual-intervention.md` ("iOS / macOS 引擎配置
64 KiB 上限"), and `EngineBudget.maxConfigBytes` changes in the same commit as the kernel rev.

A local-rules change on a running tunnel is checked against 64 KiB without the decryption section
(`next.withoutMitm()`), because the rules are swapped in place and the decryption section is not handed over again.
A decrypting tunnel can therefore keep running with rules and decryption that together are over the limit; the next
start measures the whole config again and leaves decryption out, with the reason on the requests page.

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

## Notifications

Settings › 通知 has three switches, all on by default (`NotificationSettings`): plugin script notifications
(`$notification.post`, `$notify`), unexpected disconnects (the provider stopped with a failure reason, never a
user disconnect, sleep or another VPN) and failed automatic subscription updates. The app keeps the choice in its
defaults and copies it into the tunnel profile (`TunnelProfileKeys.notifications`); a change while connected reaches
the running extension as `TunnelAppMessage.notificationSettings`, without a restart.

On iOS the extension posts script and disconnect notifications itself. The macOS tunnel is a System Extension
running as root, which cannot post, so the macOS app reads script notifications from the engine every 15 seconds
while connected and posts the disconnect notice from the stop error. Tapping a script notification opens its
`open-url` when it is an http or https link, as on the other clients (`NotificationPermission.openableURL`); an
`anixops://` link is dropped, since it would run its command (disconnect, direct mode, another node) on the tap.

The system drops every notification until the user allows them. The app asks once, the first time the tunnel is
connected while the app is in front (after the system's prompt to add the VPN configuration; never in the demo), and
when a switch is turned on. On a fresh install that is the first connect, never the launch; someone upgrading whose
tunnel is already up (On Demand) and who never answered sees it once when the app opens. While permission was never asked Settings › 通知 offers a button that asks;
when it was refused, a button that opens the system settings and a footer saying the switches do nothing until then.

## Extension log levels and modules

Every line of the extension's event log (`TunnelDiagnosticLog`) has a level (调试, 信息, 警告, 错误) and a module
(隧道, 内核, 网络, 规则, DNS, HTTPS 解密, 脚本, 内存). `record` can set them; otherwise
`DiagnosticModule.classify` derives them from the fixed message, which also covers lines from older extensions.
iOS developer mode and the macOS Activity page's "扩展日志…" filter by level-and-above and by module, and remember the
choice. The copied diagnostics report adds `[level module]` to each line.

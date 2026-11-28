# Windows Managed Client Source Release Contract

This contract documents the Windows desktop packages for the v0.2.0 Tauri integration.
The native release packages use WinUI 3, the Windows core service, and WiX 5.
See [the native installer](../../installer/windows-native/README.md) for the current package layout.
The v0.1.1-alpha.2 CLI-only ZIP is covered by
`windows-cli-artifact-source-release-contract.md`.

```text
windows-managed-client-source-release-contract=present
windows-managed-client-release-state=implementation-active
windows-managed-client-version-scope=v0.2.0
WINDOWS_CLI_ARTIFACT_GATE=windows-managed-client-active
windows-managed-client-runner=windows-latest
windows-managed-client-runner-kind=github-hosted
windows-managed-client-rust-toolchain=stable
windows-managed-client-rust-profile=minimal
windows-managed-client-target-triple=x86_64-pc-windows-gnu
windows-managed-client-package-format=msi
windows-managed-client-wix-version=4.0.6
windows-managed-client-checksum-algorithm=sha256
windows-managed-client-manifest-schema-version=2
windows-managed-client-install-model=wix-per-machine-msi
windows-managed-client-system-mutation-policy=managed-apply-and-rollback
windows-managed-client-gui=active
windows-managed-client-gui-daily-usability=home-nodes-subscriptions-settings-diagnostics-advanced-active
windows-managed-client-gui-runtime-status=in-process-engine-current-user-proxy-active
windows-managed-client-gui-background-commands=active
windows-managed-client-service=removed-legacy-service-uninstalled-by-gui-and-msi
windows-managed-client-driver-package-lifecycle=active
windows-managed-client-installer=active
windows-managed-client-diagnostics=gui-config-preflight-and-local-report-active
windows-managed-client-system-proxy-mutation=active
windows-managed-client-system-proxy-lifecycle=gui-owned-snapshot-active
windows-managed-client-trust-store-mutation=active
windows-managed-client-managed-lifecycle=active
windows-managed-client-embedded-engine=in-process-local-proxy-active
windows-managed-client-third-party-core=removed
windows-managed-client-embedded-supervision=gui-one-controlled-recovery-active
windows-managed-client-embedded-node-switch=envelope-rewrite-and-engine-restart-active
windows-managed-client-embedded-delay-test=in-process-url-request-active
windows-managed-client-embedded-urltest=background-group-health-checks-active
windows-managed-client-embedded-basic-protocols=shadowsocks-trojan-vless-vmess-hysteria2-tuic
windows-managed-client-mitm-upstream=embedded-engine-decrypts-in-process
windows-managed-client-local-profile-import=active
windows-managed-client-remote-subscription-fetch=operator-initiated-http-s-profile-import-and-update-active
windows-managed-client-remote-subscription-update=single-saved-url-explicit-only
windows-managed-client-profile-node-selector=embedded-envelope-node-and-policy-group-active
windows-managed-client-mitm-data-plane=active
windows-managed-client-mitm-certificate-lifecycle=active
windows-managed-client-mitm-protocol=embedded-engine-http1-http2
windows-managed-client-script-dispatch=blocked
windows-managed-client-authenticode-policy=unsigned-no-authenticode-with-github-attestation
windows-managed-client-attestation-policy=github-artifact-attestation-required
windows-managed-client-release-assets=enabled-after-attestation-and-publish-gate
windows-managed-client-portable-zip=active
windows-managed-client-portable-release-assets=enabled-after-attestation-and-publish-gate
windows-managed-client-webview2-loader=app-local-x64-required
windows-managed-client-webview2-runtime=system-evergreen-runtime-required
```

## Payload

The MSI contains:

- `networkcore-windows-gui.exe` from `apps/windows-gui`;
- the x64 `WebView2Loader.dll` emitted by the pinned `webview2-com-sys` build
  dependency, installed next to the GUI executable;
- `networkcore-windows.exe` from `apps/windows-cli`;
- schema-version-1 `managed-config.json` from `installer/windows`.

The MSI does not bundle or start any third-party proxy core, and it registers
no Windows service. The GUI runs the embedded engine (`engine-embedded` local
proxy, and the packet engine in TUN mode) in its own elevated process.

Versions before 0.3.0-beta.7 shipped `networkcore-windows-service.exe` and the
`AnixOpsNetworkCore` service as a second way to run the engine. That mode had
none of the debug features, so it was removed. A major upgrade removes the
service with the old package; the GUI removes one a portable copy registered
when it starts, and if that service was serving the GUI-owned proxy, restores
the proxy snapshot first.

`v0.2.0` uses a Tauri/React desktop view with a Rust-owned command bridge and
splits daily UI state, runtime observation, background command dispatch, and
theme into dedicated GUI modules. `Connected` requires the engine running in the
GUI process and the current interactive-user proxy matching the snapshot the GUI
applied; managed JSON or a persisted state record alone cannot report a
connection. The exact UI control and unavailable-feature
boundary is documented in [Windows GUI Daily Usability](windows-gui-daily-usability.md).

Import accepts an operator-selected local file, a share link, or an
operator-entered `http://`/`https://` subscription URL. The importer parses it
with `CoreSubscriptionService`, keeps every node the embedded engine can run in
`%ProgramData%\\AnixOps\\NetworkCore\\embedded\\nodes.json`, keeps subscription
rules and select/url-test/fallback policy groups in `profile.json`, and writes
the engine's configuration envelope (`envelope.json`) for the selected node,
routing mode and group choices. A URL is downloaded only for that explicit
action, with a bounded client timeout; `Update URL` fetches the saved URL
through the same pipeline, and a failed download or parse leaves the saved
source and current managed config unchanged.

Switching a node or a select group rewrites the envelope. A connected engine
on a routed profile takes the new choice in place through its `select` control
command (in TUN mode in both of its engines): open connections, the
current-user proxy and the TUN adapter stay. A single-node profile, or a choice
the running engine cannot make in place, reconnects the engine on the new
envelope. Importing, updating or selecting a subscription (and the automatic
update) reconnects a connected engine when the envelope changed, and leaves it
alone when it did not. The opt-in 30-minute monitor only moves off the current
node when it failed or another node is faster by more than the larger of 50 ms
(the engine's url-test tolerance) and 30 % of its delay; it leaves a FINAL route
that already chooses through a url-test or fallback group alone. Delay tests and `Select fastest node` run in the GUI process
against each node's own outbound (HTTP(S) URL, 5-second timeout, 16 at a time)
without changing the managed config. url-test and fallback groups run
background health checks inside the engine. Supported protocols are
Shadowsocks, Trojan, VLESS (including REALITY), VMess, Hysteria2, and TUIC with
the TLS, WebSocket, gRPC and QUIC transports the embedded engine implements.

The GUI `Enable HTTPS MITM` action generates a CA key pair below
`%ProgramData%\\AnixOps\\NetworkCore\\mitm`, then protects the fixed
`root-ca-key.pem` path before it can enter managed configuration. Its Windows
DACL has inheritance disabled and exactly two full-control ACEs: the generating
account and LocalSystem. Protection rejects reparse points, any other path, or
an ACL readback mismatch; enable removes the private key and stops before any
proxy or trust-store mutation on failure. On connect the GUI imports the CA
into LocalMachine ROOT and the embedded engine decrypts HTTPS itself on the
system proxy port, with the same MITM the iOS app uses. Disable removes the
managed ROOT entry and deletes the generated private key.

Public builds ship no built-in MITM plugin, rewrite or script
([no-presets.md](../no-presets.md)): with HTTPS decryption on, only the plugins
the user imports rewrite anything. The former general ad-block pack and the
Bilibili web trial are gone; a saved `bilibili_web_ad_block_enabled` is ignored.

When the engine started by this GUI run stops unexpectedly, the GUI makes one
controlled reconnect if `auto_recover_core` is on; disconnect, exit and a
failed connect restore the GUI-owned proxy snapshot.

The elevated GUI provides `Validate` and `Diagnostics` actions. `Validate`
parses the managed JSON with the same schema validation as connect and
checks the embedded envelope without starting a proxy, tunnel,
certificate, driver, or system-proxy mutation. `Diagnostics` writes and opens
`%ProgramData%\\AnixOps\\NetworkCore\\logs\\diagnostics.log`, containing the
managed runtime state, whether a legacy service is still registered, and
bounded, redacted tails of the local logs. URL/share links and
password/token/secret/authorization/private-key values are removed before the
report is written. Failed GUI actions generate the same report automatically
and show its path in the error dialog.

Every Windows tag release also contains a portable ZIP with the GUI, its x64
`WebView2Loader.dll`, CLI, inert `managed-config.json`, and portable README.
Extracting the ZIP registers nothing; system-mutation operations remain
explicit GUI actions.

The app-local loader prevents a missing-`WebView2Loader.dll` launch failure. It
does not embed the Microsoft Edge WebView2 Runtime: the supported Windows host
must provide the Evergreen WebView2 Runtime, which the GUI reports through its
normal launch/runtime diagnostics when unavailable.

The GUI requests UAC elevation and controls configuration import, the
current-user WinINet proxy, LocalMachine ROOT certificate entries, and signed
INF driver packages. Full MSI uninstall runs `networkcore-windows-gui.exe
--purge` as LocalSystem before `RemoveFiles`, removing the recorded certificate
and driver state and any legacy service; it logs failures and does not block
the uninstall.

`root_certificate_path` remains a separate generic trust-store lifecycle
operation. HTTPS decryption follows the embedded engine's MITM (see
`docs/debug-mode.md`). Scheduled remote subscriptions, persistent remote subscription
catalogs, and remote route/rule fetch remain unavailable.

The driver capability installs and removes a caller-configured signed INF by
using NewDev `DiInstallDriverW` and `DiUninstallDriverW`. A kernel driver binary
is not built by this repository and is not accepted unless Windows validates
the package signature.

## CI And Release

GitHub Actions builds all Rust binaries for `x86_64-pc-windows-gnu`, pins WiX
4.0.6, builds and validates the MSI, performs a bounded real MSI
install/uninstall smoke that checks no service is registered and that the
uninstall purge removes a legacy `AnixOpsNetworkCore` service, creates SHA-256 and schema-version-2 manifest files
for the MSI and portable ZIP, and attests all eight Windows release-bundle
files before publication. No local build, test, installer, or release
validation is permitted.

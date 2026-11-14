# NetworkCore Windows Installer

`Package.wxs` is the WiX 4.0.6 source for the per-machine NetworkCore MSI.
GitHub Actions supplies the compiled GUI and CLI paths plus a numeric MSI
product version at build time.

The installer:

- installs `networkcore-windows-gui.exe` and `networkcore-windows.exe` under
  Program Files;
- preserves `managed-config.json` under ProgramData across upgrades;
- creates an AnixOps NetworkCore Start Menu shortcut;
- runs `networkcore-windows-gui.exe --purge` as LocalSystem before a full
  uninstall so the root certificates and driver the client installed are
  removed (the outcome goes to `gui.log`; a failure does not block uninstall).

The client has no Windows service. Versions before 0.3.0-beta.7 registered
`AnixOpsNetworkCore`: upgrading over such an MSI removes it with the old
package, and the GUI removes one a portable copy registered the next time it
starts, putting back the proxy it served.

## Portable package

Each Windows tag release also includes a portable ZIP and its SHA-256 and
manifest files. Extract the ZIP, keep its files together, and run
`networkcore-windows-gui.exe`. Extraction does not register anything.

## First run and configuration

The GUI runs the embedded engine in its own process (it always runs elevated).
Home `Connect` starts the engine, waits for its loopback listener and only then
applies the configured proxy for the interactive user and atomically saves that
snapshot. The snapshot is restored on disconnect, on exit and after a failure.
The manual GUI `Restore network settings` action restores only a snapshot owned
by that GUI session; it cannot enable an unverified proxy endpoint.
The shipped `managed-config.json` deliberately contains only `null` values.
That default is safe and does not change the system proxy, certificate store,
or driver state. The GUI writes this file when a subscription is
imported; it lives at:

`C:\ProgramData\AnixOps\NetworkCore\managed-config.json`

A configuration for the embedded engine as a local proxy looks like:

```json
{
  "schema_version": 2,
  "system_proxy": {
    "enabled": true,
    "server": "127.0.0.1:7890",
    "bypass": "<local>"
  },
  "system_proxy_owner": "desktop",
  "root_certificate_path": null,
  "driver_package": null,
  "native_mitm": null,
  "embedded": {
    "enabled": true,
    "envelope_path": "C:\\ProgramData\\AnixOps\\NetworkCore\\embedded\\envelope.json",
    "listen_host": "127.0.0.1",
    "listen_port": 7890
  }
}
```

The GUI owns the exact current-user proxy snapshot and rollback;
`system_proxy_owner` is written as `"desktop"`, and `"service"` from older
versions is read the same way. `envelope_path` is the embedded
engine's configuration envelope; the GUI generates it from the imported
subscription, the selected node, the routing mode and the policy-group choices.
No third-party proxy binary is downloaded or started.

## GUI profile workflow

1. Enter a subscription URL, a share link, or a local file on the Proxies page
   and import it. The GUI saves the parsed nodes under
   `%ProgramData%\AnixOps\NetworkCore\embedded` and writes the envelope.
2. Use Home `Connect`.
3. Switch nodes, run delay tests, or pick the fastest node; a connected engine
   restarts on the new choice. Subscription rules and policy groups appear on
   the Rules page, where select groups can be switched.

Accepted inputs are `ss://`, `trojan://`, `vless://`, `vmess://`,
`hysteria2://`/`hy2://` and `tuic://` links, Clash YAML `proxies` (with `rules`
and `proxy-groups`), JSON outbound lists, and Surge, Loon or Quantumult X
proxy lines. A node the embedded engine cannot run is reported on import.

`Enable HTTPS MITM` creates the client's own root CA, trusts it, and the
embedded engine decrypts HTTPS itself on the system proxy port (`127.0.0.1:7890`),
in TUN mode as well. `Disable HTTPS MITM` removes the CA again.

The optional fields are:

- `root_certificate_path`: an existing certificate file that Windows CryptoAPI
  can decode, for example `C:\ProgramData\AnixOps\NetworkCore\root-ca.cer`.
- `driver_package.inf_path`: the entry-point INF of a signed driver package.
- A `tunnel` field left by an older version is ignored.
- `embedded`: the embedded engine's envelope path and loopback listener.
- `native_mitm`: the CA certificate/key paths and the listener layout HTTPS
  decryption uses. The GUI writes this block when `Enable HTTPS MITM` is
  selected.

`root_certificate_path` only imports an existing certificate into the Windows
LocalMachine ROOT store. The GUI HTTPS MITM action instead creates its own CA.

The GUI writes diagnostics to `C:\ProgramData\AnixOps\NetworkCore\logs\gui.log`.
`Diagnostics` creates and opens `diagnostics.txt`, containing runtime status
and bounded local log tails; an action failure writes the same report and shows
its path. `networkcore-windows-gui.exe --debug` adds verbose GUI activity
records only.

The MSI and portable ZIP are built and validated only by GitHub Actions. CI
also performs a bounded silent MSI install/uninstall smoke test and checks
that no Windows service is registered. Do not invoke WiX or Windows Installer locally.

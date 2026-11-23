# NetworkCore Windows Installer

`Package.wxs` is the WiX 4.0.6 source for the per-machine NetworkCore MSI.
GitHub Actions supplies the compiled GUI and CLI paths plus a numeric MSI
product version at build time.

The installer:

- installs `networkcore-windows-gui.exe` and `networkcore-windows.exe` under
  Program Files, and remembers the folder so that an upgrade installs over it;
- creates `%ProgramData%\AnixOps\NetworkCore` open only to SYSTEM and
  Administrators (the GUI closes it again at every start, see below);
- creates an AnixOps NetworkCore Start Menu shortcut;
- on a full uninstall (not on an upgrade) runs, as LocalSystem,
  `networkcore-windows-gui.exe --cleanup-startup`, which removes every user's
  sign-in startup task, and `networkcore-windows-gui.exe --purge`, which
  removes:
  - the HTTPS decryption roots trusted in the own certificate store
    (`Cert:\CurrentUser\Root`) of every user signed in at the time: those
    noted in `trusted-roots.txt` when a user trusted one, and the current root.
    A user who is not signed in keeps the root and removes it in `certmgr.msc`;
  - the decryption root's private key (`mitm\root-ca-key.pem`);
  - the driver installed from the settings page and the root certificates and
    driver older versions recorded;
  - the Windows service versions before 0.3.0-beta.7 registered;
  - the system proxy the GUI set and could not put back itself (it was killed,
    or was connected when the removal began), from its owner record
    `owned-proxy.json`: the machine's WinHTTP proxy, and the WinINet settings
    of each recorded user who is signed in, each only while it is still the
    value the GUI applied. A user who is not signed in keeps the record; the
    GUI puts their proxy back if it is installed and started again, otherwise
    they turn the proxy off in Windows Settings › Network & internet › Proxy.

  The outcome goes to `logs\gui.log` (`purged: ...`); a failure does not block
  the uninstall. The rest of the data folder (subscriptions, settings, logs)
  stays, closed to ordinary users.

### Upgrades

The MSI removes the earlier version only after installing the new one
(`MajorUpgrade Schedule="afterInstallExecute"`). Files both versions install
keep their reference, so `managed-config.json`, which the GUI rewrites when a
subscription is imported or HTTPS decryption is turned on, is kept as it is;
sign-in startup tasks, trusted decryption roots and the key are kept too. This
needs two rules for every release:

- no component changes its identity (Id, folder, file name, key path);
- every versioned file has a higher file version than in the previous release.
  `release.yml` stamps the MSI version into the GUI executable's file version
  (`major.minor.patch.msiBuild`) and fails when the built executable does not
  carry it; the CLI has no version resource and is replaced by content.
  Bundled third-party DLLs (`WebView2Loader.dll`, `wintun.dll`) keep their
  own file versions: an upgrade replaces them only when that version rises.
  A release that ships a different build of one of them under the same file
  version leaves the old file in place, and the GUI's start-up integrity check
  then fails; such a DLL needs a new file version (or a new file name and
  component) before it ships.

The betas' packages did not tell an upgrade from a removal. When a beta is
upgraded, its uninstall actions run the new version's GUI, which sees two
registered versions of the package and skips (`--purge skipped: the package is
being upgraded` in `gui.log`). An upgrade from a beta whose
`managed-config.json` was already reset, or whose sign-in task was removed, is
repaired the next time the GUI starts: the imported profile's engine block is
written back from `embedded\envelope.json` (HTTPS decryption stays off until
the user turns it on again), and the logon task of a user who chose "start
after sign-in" is registered again. That choice is kept per user
(`start_after_login_users` in `desktop-state.json`). The betas kept one
switch for the whole computer: on a computer with several administrators, the
first one to start 1.0 takes that switch over as their own choice, and the
others choose again on the settings page.

CI builds the same source twice with different file versions, installs the
older MSI (with the betas' uninstall conditions), upgrades to the newer one and
checks that `managed-config.json`, the sign-in task, a trusted root and the
key are kept and the GUI was replaced; then it uninstalls and checks that the
task, the root, the key and the legacy service are gone.

### Data folder access

`%ProgramData%\AnixOps\NetworkCore` holds subscription addresses with their
tokens, node credentials, device keys and the decryption key. `%ProgramData%`
lets every user read what is made in it unless told otherwise, so before it
reads or writes anything the GUI closes the folder (owner Administrators,
protected access list: SYSTEM and Administrators in full control). The first
start after an upgrade also resets everything in it, removes links and removes
what someone other than SYSTEM, Administrators or the user running the GUI
owns (an ordinary user could have put it there for the elevated GUI to read),
then writes `.access-closed`; later starts only check the folder. Each entry is
opened once as itself (never through a link) and checked, reset or removed
through that handle: symbolic links, junctions and files with more than one
name (hard links, whose other name may be outside the folder) are removed by
that name only, and what is removed keeps its owner and access list, so nothing
outside the folder is changed. Compressed, deduplicated and cloud files are
reparse points too but hold their own data, so they are kept like any other
file. The folder must also be where its path says, with no link on the way,
and every entry is checked to be inside it (by the path its open handle
resolves to) before it is reset or removed, so a folder swapped for a link
during the walk stops it instead of redirecting it. `.access-closed` is made
by its own handle, never through a link, and checked to be in the folder; a
marker that is a link or has a second name stops the start. The folder above,
`%ProgramData%\AnixOps`, must be a real folder (not a link or other reparse
point). If someone other than SYSTEM, Administrators or TrustedInstaller owns
it, or it lets others delete, rename or change the access of what is in it,
the GUI takes it over through the same handle (owner Administrators,
protected access list: SYSTEM and Administrators in full control, Users read
and traverse only) and does not start when that fails. The installer's
`PermissionEx` SDDL for the data folder sets no owner, and the installer
leaves the access of an existing `%ProgramData%\AnixOps` as it is, so a data
folder or folder above it that a standard user made first (for example on a
machine that ran a beta) keeps that user as owner after installing; the client
takes both over on its first start.
The folder and the one above it are never removed and are opened without
asking to, so a console whose current folder is one of them, or a backup or
virus scan reading them, does not stop the GUI from starting; a file the
client keeps that such a program holds open is still reset. If the folder
cannot be closed, the GUI shows the error and does not start. Device keys and
the decryption key are also written closed to everyone but their owner,
SYSTEM and Administrators.

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
snapshot. Before the proxy changes, the GUI writes an owner record
(`owned-proxy.json`: the user, the GUI process and its start time, the applied
proxy and the settings to put back). The snapshot is restored on disconnect, on
exit (tray Quit or any other end of the application), when Windows signs the
user out, shuts down or asks the GUI to close for an installer, and after a
failed connection. When the GUI could not do it (a crash, the process ended in
Task Manager, a forced shutdown), its next start — also from the sign-in task —
restores it when the owner process is gone and the settings are still the ones
it applied, tells the user in a notification, and only then connects
automatically; settings the user changed since are left alone. The user's
WinINet settings and the machine's WinHTTP proxy are judged apart, so when the
user has already turned their own proxy off, the WinHTTP proxy still goes back. Disconnecting
reads no configuration, so a missing or invalid `managed-config.json` never
keeps the proxy pointing at a stopped engine. A `desktop-state.json` that
cannot be read is moved aside to `desktop-state.json.broken-<unix seconds>` and
the GUI starts with the defaults and a notification instead of not starting.
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

`Enable HTTPS MITM` uses the client's own root CA (created the first time the
user asks to trust it) and the embedded engine decrypts HTTPS itself on the
system proxy port (`127.0.0.1:7890`), in TUN mode as well. The client never adds
the root to a trust store: the user trusts it by hand in Windows' certificate
dialog, into their own store, and the client notes its thumbprint so the
uninstaller can remove it. `Disable HTTPS MITM` keeps the root and its key, so
turning decryption on again needs no new trust; remove the root in
`certmgr.msc` when you stop using decryption, or uninstall the client.

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
also performs bounded silent MSI install, upgrade and uninstall tests (see
Upgrades) and checks that no Windows service is registered. Do not invoke WiX or Windows Installer locally.

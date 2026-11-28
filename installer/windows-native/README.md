# AnixOps Net for Windows: installer

`Package.wxs` is the WiX 5 source of the per-machine MSI of the native Windows client
(`docs/architecture/desktop-native.md`). CI builds it in the "Windows native app" job from the published
window (`apps/windows-native`, self-contained) and the core service (`apps/windows-service`), then installs,
upgrades and removes it on the runner (`.github/scripts/native-msi-lifecycle.ps1`).

It installs beside the web client (AnixOps NetworkCore): its own upgrade code, its own folder
(`Program Files\AnixOps\Net`), its own service and its own data folder.

What it does:

- Installs the window `AnixOpsNet.exe` with its runtime and a Start menu shortcut. The window runs as the
  signed-in user and never asks for administrator rights.
- Installs `anixops-net-service.exe` as the service `AnixOpsNetCore`: LocalSystem, starts with Windows, and is
  started at the end of the installation.
- Upgrade: the old version is removed first (service stopped and deleted, open windows closed), then the new
  one is installed and its service started. The users' login entries and the client's data stay.
- Removal: the service is stopped and deleted, open windows are closed, the files and the shortcut are
  removed, and the entry that starts the window after login and the HTTPS decryption root are taken out
  of every signed-in user's settings. A user who is not signed in at that moment keeps a login entry that points at nothing; Windows
  ignores it.
- The client's data under `%ProgramData%\AnixOps\Net` (subscriptions, settings, logs) is kept on removal. It
  is closed to ordinary users.

While the service stops it has the open window put the user's proxy settings back; only then are the windows
closed (`apps/windows-service/src/cleanup.rs`).

TUN mode: `wintun.dll` (WireGuard's signed build, unchanged, with `wintun-LICENSE.txt`) is installed beside
the service, which loads it by that path. Wintun installs its driver the first time an adapter is made; on
removal `anixops-net-service --purge` has Wintun delete the driver again, which Wintun does only when no
adapter of any program still uses it.

HTTPS decryption: the installer trusts no certificate. When a user asks for it, that user's window adds
the client's root to the user's own certificate store (Windows asks to confirm), and the service notes its
thumbprint in `trusted-roots.txt` in its data folder. On removal `anixops-net-service --purge` takes the
noted roots out of the signed-in users' stores again.

The release workflow builds this MSI and a portable ZIP, with checksums and GitHub build attestations.
Both include the licence and third-party notices. The MSI uses the AnixOps Net app icon.
The portable ZIP runs the core in the foreground as described in `PORTABLE-README.txt`.
The MSI ships without Authenticode signing.

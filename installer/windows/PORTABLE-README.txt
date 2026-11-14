AnixOps NetworkCore Windows portable package

1. Extract the entire directory to a writable location.
2. Run networkcore-windows-gui.exe and approve the administrator prompt. The
   client always runs elevated (TUN mode, certificate and driver operations
   need it); "Start after sign-in" uses an elevated logon task, so no prompt
   appears at sign-in.
   WebView2Loader.dll is included beside the GUI executable. Windows must still
   have the Microsoft Edge WebView2 Evergreen Runtime installed.
   wintun.dll (Wintun 0.14.1, see wintun-LICENSE.txt) is included beside the
   GUI executable for TUN mode; keep it in the same directory.
3. On the Proxies page, enter a subscription URL, a share link or a local file
   and import it. The GUI saves the parsed nodes and writes the engine's
   configuration.
4. Use Home Connect. The engine runs inside the GUI process; the GUI applies
   the current user's proxy only after the engine's loopback listener answers,
   and restores the previous proxy on disconnect, on exit and after a failure.

The client has no Windows service. Versions before 0.3.0-beta.7 could register
one ("AnixOps NetworkCore"); the GUI removes it the next time it starts.

The portable ZIP registers nothing during extraction. It is included with
every Windows tag release alongside the MSI and its own SHA-256 and manifest
files.

"Start NetworkCore after I sign in" in Settings is off by default and points
to the extracted GUI executable, so disable it in Settings before moving or
deleting the portable directory.

Managed configuration, state, and logs are stored under:

  %ProgramData%\AnixOps\NetworkCore

Before deleting the extracted directory, remove the certificates and driver the
client installed by running this command from an Administrator terminal:

  networkcore-windows-gui.exe --purge

The outcome is written to %ProgramData%\AnixOps\NetworkCore\logs\gui.log.

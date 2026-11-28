AnixOps Net for Windows (portable)

Verify the ZIP against the release checksum, then extract it to a local folder
you control. To run the core without installing a Windows service, open
PowerShell in that folder and run:

    .\anixops-net-service.exe --console --dir "$env:LOCALAPPDATA\AnixOps\Net-Portable" --pipe '\\.\pipe\AnixOpsNetPortable'

Leave that window open. From a separate PowerShell window in the same folder,
start the desktop app as your signed-in user:

    .\AnixOpsNet.exe --pipe AnixOpsNetPortable

TUN requires an elevated core. Use the MSI if you need an installed service
with persistent startup and system integration.

Before leaving, disconnect in the app and close it, then stop the core with
Ctrl+C. To remove the portable app, delete the extracted folder. Your
subscriptions and settings stay in %LOCALAPPDATA%\AnixOps\Net-Portable.

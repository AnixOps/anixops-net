AnixOps Net for Windows (portable)

Verify the ZIP against the release checksum and extract it to a local folder
that only administrators can modify. The foreground core needs administrator
rights to secure its data folder and to run TUN. Open PowerShell as an
administrator in the extracted folder and run:

    .\anixops-net-service.exe --console --dir "$env:ProgramData\AnixOps\Net-Portable" --pipe '\\.\pipe\AnixOpsNetPortable'

Leave that window open. From a separate PowerShell window in the same folder,
start the desktop app as your signed-in user:

    .\AnixOpsNet.exe --pipe AnixOpsNetPortable

Use the MSI if you need an installed service with persistent startup and system
integration.

Before leaving, disconnect in the app and close it, then stop the core with
Ctrl+C. To remove the portable app, delete the extracted folder. Your
subscriptions and settings stay in %ProgramData%\AnixOps\Net-Portable.

# Checks a built Windows MSI and the executables in it carry the app icon (.github/scripts/windows_icons.py checks
# the sources): every row of the Shortcut table names an icon, that icon is in the Icon table, ARPPRODUCTICON
# names one too, and each executable given has an icon group resource. An advertised shortcut without an icon
# shows the blank file icon in the Start menu and in search.
param(
    [Parameter(Mandatory = $true)][string]$Msi,
    [string[]]$Exe = @()
)

$ErrorActionPreference = "Stop"

function Get-MsiRows([object]$Database, [string]$Query, [int]$Columns) {
    $view = $Database.GetType().InvokeMember("OpenView", "InvokeMethod", $null, $Database, @($Query))
    $view.GetType().InvokeMember("Execute", "InvokeMethod", $null, $view, $null) | Out-Null
    $rows = @()
    while ($true) {
        $record = $view.GetType().InvokeMember("Fetch", "InvokeMethod", $null, $view, $null)
        if ($null -eq $record) { break }
        $row = @()
        for ($i = 1; $i -le $Columns; $i++) {
            $row += $record.GetType().InvokeMember("StringData", "GetProperty", $null, $record, @($i))
        }
        $rows += , $row
    }
    $view.GetType().InvokeMember("Close", "InvokeMethod", $null, $view, $null) | Out-Null
    return , $rows
}

$installer = New-Object -ComObject WindowsInstaller.Installer
$database = $installer.GetType().InvokeMember("OpenDatabase", "InvokeMethod", $null, $installer, @((Resolve-Path $Msi).Path, 0))
$icons = @((Get-MsiRows $database "SELECT ``Name`` FROM ``Icon``" 1) | ForEach-Object { $_[0] })
if ($icons.Count -eq 0) { throw "$Msi has no Icon table rows." }
$shortcuts = Get-MsiRows $database "SELECT ``Shortcut``, ``Icon_`` FROM ``Shortcut``" 2
if ($shortcuts.Count -eq 0) { throw "$Msi has no shortcuts." }
foreach ($shortcut in $shortcuts) {
    if (-not $shortcut[1]) { throw "$Msi shortcut $($shortcut[0]) has no icon: it would show the blank file icon." }
    if ($icons -notcontains $shortcut[1]) { throw "$Msi shortcut $($shortcut[0]) names icon $($shortcut[1]), which is not in the Icon table." }
}
$arp = Get-MsiRows $database "SELECT ``Value`` FROM ``Property`` WHERE ``Property`` = 'ARPPRODUCTICON'" 1
if ($arp.Count -ne 1 -or $icons -notcontains $arp[0][0]) { throw "$Msi has no ARPPRODUCTICON naming an icon in the Icon table." }
[System.Runtime.InteropServices.Marshal]::ReleaseComObject($database) | Out-Null
Write-Host "$Msi`: $($shortcuts.Count) shortcut(s) and the Installed apps entry carry an icon ($($icons -join ', '))."

Add-Type -Namespace IconCheck -Name Native -MemberDefinition @"
[DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
public static extern IntPtr LoadLibraryEx(string path, IntPtr file, uint flags);
[DllImport("kernel32.dll")]
public static extern bool FreeLibrary(IntPtr module);
public delegate bool EnumNames(IntPtr module, IntPtr type, IntPtr name, IntPtr param);
[DllImport("kernel32.dll", CharSet = CharSet.Unicode)]
public static extern bool EnumResourceNames(IntPtr module, IntPtr type, EnumNames callback, IntPtr param);
"@

foreach ($path in $Exe) {
    $full = (Resolve-Path $path).Path
    # LOAD_LIBRARY_AS_DATAFILE | LOAD_LIBRARY_AS_IMAGE_RESOURCE: only the resources are read, nothing runs.
    $module = [IconCheck.Native]::LoadLibraryEx($full, [IntPtr]::Zero, 0x22)
    if ($module -eq [IntPtr]::Zero) { throw "Could not open $full to read its resources." }
    $script:groups = 0
    $callback = [IconCheck.Native+EnumNames] { param($m, $t, $n, $p) $script:groups++; return $true }
    # RT_GROUP_ICON = 14
    [IconCheck.Native]::EnumResourceNames($module, [IntPtr]14, $callback, [IntPtr]::Zero) | Out-Null
    [IconCheck.Native]::FreeLibrary($module) | Out-Null
    if ($script:groups -lt 1) { throw "$full has no icon resource: Explorer and the taskbar would show the default program icon." }
    Write-Host "$full`: $($script:groups) icon group resource(s)."
}

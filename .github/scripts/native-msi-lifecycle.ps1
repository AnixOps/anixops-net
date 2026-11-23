# Installs, upgrades and removes the native Windows client's MSI on this machine and checks what each step
# leaves behind (docs/architecture/desktop-native.md, pass mark 3). Run elevated, as CI does.
param(
  [Parameter(Mandatory = $true)][string]$First,
  [Parameter(Mandatory = $true)][string]$Second,
  [Parameter(Mandatory = $true)][string]$SecondVersion,
  [Parameter(Mandatory = $true)][string]$TestProject,
  [Parameter(Mandatory = $true)][string]$SessionTestProject
)
$ErrorActionPreference = "Stop"

$serviceName = "AnixOpsNetCore"
$installDir = Join-Path $env:ProgramFiles "AnixOps\Net"
$dataDir = Join-Path $env:ProgramData "AnixOps\Net"
$window = Join-Path $installDir "AnixOpsNet.exe"
$service = Join-Path $installDir "anixops-net-service.exe"
$shortcut = Join-Path $env:ProgramData "Microsoft\Windows\Start Menu\Programs\AnixOps Net.lnk"
$runKey = "HKCU:\Software\Microsoft\Windows\CurrentVersion\Run"
$runValue = "AnixOpsNet"

function Invoke-Msi {
  param([string[]]$Arguments, [string]$Operation)
  $log = Join-Path $env:RUNNER_TEMP "anixops-net-msi-$Operation.log"
  $all = $Arguments + @("/qn", "/norestart", "/l*v", "`"$log`"")
  $process = Start-Process -FilePath "msiexec.exe" -ArgumentList $all -PassThru
  if (-not $process.WaitForExit(300000)) {
    Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
    if (Test-Path $log) { Get-Content $log -Tail 200 }
    throw "$Operation took more than 300 seconds."
  }
  # 3010 would mean something is only gone after a restart, which is a leftover until then.
  if ($process.ExitCode -ne 0) {
    if (Test-Path $log) { Get-Content $log -Tail 200 }
    throw "$Operation ended with exit code $($process.ExitCode)."
  }
  Write-Host "${Operation}: ok"
}

function Get-Products {
  @(Get-ItemProperty "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*" -ErrorAction SilentlyContinue |
      Where-Object { $_.PSObject.Properties["DisplayName"] -and $_.DisplayName -eq "AnixOps Net" })
}

function Get-RunEntry {
  (Get-ItemProperty -Path $runKey -Name $runValue -ErrorAction SilentlyContinue).$runValue
}

function Assert-Installed {
  param([string]$Version)
  $registered = Get-CimInstance Win32_Service -Filter "Name='$serviceName'"
  if ($null -eq $registered) { throw "The service is not registered." }
  if ($registered.State -ne "Running") { throw "The service is $($registered.State), not running." }
  if ($registered.StartMode -ne "Auto") { throw "The service starts '$($registered.StartMode)', not with Windows." }
  if ($registered.StartName -ne "LocalSystem") { throw "The service runs as $($registered.StartName)." }
  if ($registered.PathName.Trim('"') -ne $service) { throw "The service runs $($registered.PathName)." }
  # Restarted when it fails, also when it stops itself with an error code rather than crashing.
  $failure = (& sc.exe qfailure $serviceName) -join "`n"
  if ($failure -notmatch "RESTART") { throw "The service is not restarted when it fails: $failure" }
  $failureFlag = (& sc.exe qfailureflag $serviceName) -join "`n"
  if ($failureFlag -notmatch "FAILURE_ACTIONS_ON_NONCRASH_FAILURES\s*:\s*TRUE") { throw "The service is not restarted after a non-crash failure: $failureFlag" }
  foreach ($file in @($window, $service, $shortcut, (Join-Path $installDir "wintun.dll"), (Join-Path $installDir "wintun-LICENSE.txt"))) {
    if (-not (Test-Path $file -PathType Leaf)) { throw "$file is not there." }
  }
  $products = Get-Products
  if ($products.Count -ne 1) { throw "$($products.Count) products named AnixOps Net are registered." }
  if ($products[0].DisplayVersion -ne $Version) { throw "Version $($products[0].DisplayVersion) is registered, expected $Version." }
  Write-Host "installed ${Version}: service running as LocalSystem, files and shortcut in place"
}

function Assert-WindowReachesService {
  # The installed window on the installed service's own pipe, through UI Automation.
  $env:ANIXOPS_NET_APP_EXE = $window
  $env:ANIXOPS_NET_INSTALLED = "1"
  try {
    dotnet test $TestProject --configuration Release --no-build --filter "FullyQualifiedName~InstalledTests" --logger "console;verbosity=normal"
    if ($LASTEXITCODE -ne 0) { throw "The installed window did not reach the installed service." }
  } finally {
    Remove-Item Env:ANIXOPS_NET_APP_EXE
    Remove-Item Env:ANIXOPS_NET_INSTALLED
  }
}

function Assert-TunModeWorks {
  # TUN mode against the installed service, which runs as LocalSystem and loads the installed wintun.dll.
  $env:ANIXOPS_NET_TUN_TESTS = "1"
  $env:ANIXOPS_NET_INSTALLED = "1"
  try {
    dotnet test $SessionTestProject --configuration Release --no-build --filter "FullyQualifiedName~TunModeTests" --logger "console;verbosity=normal"
    if ($LASTEXITCODE -ne 0) { throw "TUN mode did not work through the installed service." }
  } finally {
    Remove-Item Env:ANIXOPS_NET_TUN_TESTS
    Remove-Item Env:ANIXOPS_NET_INSTALLED
  }
}

function Test-TunDriver {
  # The driver package Wintun puts into the driver store the first time an adapter is made.
  $drivers = & (Join-Path $env:SystemRoot "System32\pnputil.exe") /enum-drivers
  [bool]($drivers | Select-String -SimpleMatch "wintun.inf")
}

function Assert-RootGetsTrusted {
  # The installed service has this user's window trust its HTTPS decryption root; the test answers the
  # confirmation Windows shows.
  $env:ANIXOPS_NET_ROOT_TESTS = "1"
  $env:ANIXOPS_NET_INSTALLED = "1"
  try {
    dotnet test $TestProject --configuration Release --no-build --filter "FullyQualifiedName~RootStoreTests" --logger "console;verbosity=normal"
    if ($LASTEXITCODE -ne 0) { throw "The installed service's root was not trusted in the user's store." }
  } finally {
    Remove-Item Env:ANIXOPS_NET_ROOT_TESTS
    Remove-Item Env:ANIXOPS_NET_INSTALLED
  }
}

function Get-TrustedRoots {
  # The client's root in this user's own store and in the machine's.
  @(Get-ChildItem Cert:\CurrentUser\Root, Cert:\LocalMachine\Root |
      Where-Object { $_.Subject -like "*AnixOps Net HTTPS Decryption Root*" })
}

function Start-Window {
  $started = Start-Process -FilePath $window -PassThru
  Start-Sleep -Seconds 5
  if ($started.HasExited) { throw "The installed window exited with code $($started.ExitCode)." }
}

if ($null -ne (Get-Service -Name $serviceName -ErrorAction SilentlyContinue)) { throw "The service is registered before the test." }

# 1. Install.
Invoke-Msi -Arguments @("/i", "`"$First`"") -Operation "install"
Assert-Installed -Version "0.3.0"
if (-not (Test-Path $dataDir -PathType Container)) { throw "The service did not create its data folder." }
Assert-WindowReachesService

# What a user of this installation has: a window open, the login entry, and data.
Start-Window
New-ItemProperty -Path $runKey -Name $runValue -Value "`"$window`" --start-after-login" -PropertyType String -Force | Out-Null
$marker = Join-Path $dataDir "upgrade-marker.txt"
Set-Content -Path $marker -Value "kept"

# 2. Upgrade with the window open.
Invoke-Msi -Arguments @("/i", "`"$Second`"") -Operation "upgrade"
Assert-Installed -Version $SecondVersion
if (@(Get-Process -Name "AnixOpsNet" -ErrorAction SilentlyContinue).Count -ne 0) { throw "The upgrade left the old window running." }
if (-not (Get-RunEntry)) { throw "The upgrade removed the user's login entry." }
if (-not (Test-Path $marker)) { throw "The upgrade removed the client's data." }
Assert-WindowReachesService

# 3. TUN mode through the installed service; what it leaves on the machine is the driver.
Assert-TunModeWorks
if (-not (Test-TunDriver)) { throw "TUN mode worked, but no Wintun driver is in the driver store; the check after removal would prove nothing." }

# 4. The user trusts the HTTPS decryption root: it goes into this user's own store, never the machine's.
if ((Get-TrustedRoots).Count -ne 0) { throw "A client root is trusted before the test." }
Assert-RootGetsTrusted
$trusted = Get-TrustedRoots
if ($trusted.Count -ne 1) { throw "$($trusted.Count) client roots are trusted, expected the one in the user's store." }
if ($trusted[0].PSParentPath -notlike "*CurrentUser*") { throw "The root was trusted in $($trusted[0].PSParentPath), not in the user's own store." }

# 5. Remove with the window open.
Start-Window
Invoke-Msi -Arguments @("/x", "`"$Second`"") -Operation "uninstall"
if ($null -ne (Get-Service -Name $serviceName -ErrorAction SilentlyContinue)) { throw "The removal left the service registered." }
foreach ($name in @("AnixOpsNet", "anixops-net-service")) {
  if (@(Get-Process -Name $name -ErrorAction SilentlyContinue).Count -ne 0) { throw "The removal left $name running." }
}
if (Test-Path $installDir) {
  $left = (Get-ChildItem $installDir -Recurse -Force | Select-Object -First 20 -ExpandProperty FullName) -join ", "
  throw "The removal left files in ${installDir}: $left"
}
if (Test-Path $shortcut) { throw "The removal left the Start menu shortcut." }
if ($null -ne (Get-NetAdapter -Name "AnixOps Net" -ErrorAction SilentlyContinue)) { throw "The removal left the TUN adapter." }
if (Test-TunDriver) { throw "The removal left the Wintun driver in the driver store." }
if ((Get-TrustedRoots).Count -ne 0) { throw "The removal left the HTTPS decryption root trusted." }
if (Get-RunEntry) { throw "The removal left the user's login entry." }
if ((Get-Products).Count -ne 0) { throw "The removal left the product registered." }
# The client's data is the user's and is kept on purpose (subscriptions, settings).
if (-not (Test-Path $marker)) { throw "The removal deleted the client's data." }
Remove-Item -Path $dataDir -Recurse -Force -ErrorAction SilentlyContinue
Write-Host "removed: no service, no process, no files, no shortcut, no login entry, no TUN adapter, no TUN driver, no trusted root; data kept"

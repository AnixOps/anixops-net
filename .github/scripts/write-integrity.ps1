# Writes the install integrity list the Windows client checks at start (crates/client-health, integrity.rs):
# one "<sha256>  <file name>" line per installed file, file names relative to the client executable.
#
# Usage: write-integrity.ps1 -Output <path> -Entry "networkcore-windows-gui.exe=<path>", ...
param(
  [Parameter(Mandatory = $true)][string]$Output,
  [Parameter(Mandatory = $true)][string[]]$Entry
)
$ErrorActionPreference = "Stop"
$lines = foreach ($item in $Entry) {
  $name, $path = $item -split "=", 2
  if (-not $name -or -not $path -or -not (Test-Path -Path $path -PathType Leaf)) {
    throw "Integrity entry is not name=<existing file>: $item"
  }
  $hash = (Get-FileHash -Algorithm SHA256 -Path $path).Hash.ToLowerInvariant()
  "$hash  $name"
}
New-Item -ItemType Directory -Force -Path (Split-Path -Parent $Output) | Out-Null
$lines | Set-Content -Path $Output -Encoding ascii
Write-Output "Integrity list $Output"
$lines | Write-Output

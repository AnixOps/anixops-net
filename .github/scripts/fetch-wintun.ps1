# Downloads the Wintun prebuilt package, checks its pinned SHA-256 and copies
# the amd64 wintun.dll (unmodified, as its license requires) plus its license
# to the destination directory. Prints the DLL path.
param(
  [Parameter(Mandatory = $true)][string]$Destination
)
$ErrorActionPreference = "Stop"
$version = "0.14.1"
$expectedSha256 = "07c256185d6ee3652e09fa55c0b673e2624b565e02c4b9091c79ca7d2f24ef51"
$work = Join-Path ([System.IO.Path]::GetTempPath()) "wintun-$version-$([guid]::NewGuid())"
New-Item -ItemType Directory -Force -Path $work | Out-Null
$zip = Join-Path $work "wintun.zip"
Invoke-WebRequest -Uri "https://www.wintun.net/builds/wintun-$version.zip" -OutFile $zip -UseBasicParsing
$actual = (Get-FileHash -Algorithm SHA256 -Path $zip).Hash.ToLowerInvariant()
if ($actual -ne $expectedSha256) {
  throw "wintun-$version.zip SHA-256 mismatch: expected $expectedSha256, got $actual"
}
Expand-Archive -Path $zip -DestinationPath $work -Force
New-Item -ItemType Directory -Force -Path $Destination | Out-Null
$dll = Join-Path $Destination "wintun.dll"
Copy-Item -Path (Join-Path $work "wintun\bin\amd64\wintun.dll") -Destination $dll -Force
Copy-Item -Path (Join-Path $work "wintun\LICENSE.txt") -Destination (Join-Path $Destination "wintun-LICENSE.txt") -Force
if ((Get-Item $dll).Length -le 0) {
  throw "wintun.dll is empty"
}
(Resolve-Path $dll).Path

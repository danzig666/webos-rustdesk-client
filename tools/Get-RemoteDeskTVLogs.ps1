<#
.SYNOPSIS
  Copy RemoteDesk TV's log and diagnostic report from the TV to the current folder.
  The files contain no passwords, tokens or typed text.

.EXAMPLE
  .\Get-RemoteDeskTVLogs.ps1 -TvHost 192.168.1.50 -Route Rooted
.EXAMPLE
  .\Get-RemoteDeskTVLogs.ps1 -TvHost 192.168.1.50 -Route DevMode -KeyFile $env:USERPROFILE\.ssh\tv_webos
#>
param(
  [Parameter(Mandatory = $true)][string]$TvHost,
  [ValidateSet('DevMode', 'Rooted')][string]$Route = 'Rooted',
  [string]$KeyFile
)
$ErrorActionPreference = 'Stop'
$Dir = '/media/developer/apps/usr/palm/applications/io.github.danzig666.remotedesktv/data'
$files = @("$Dir/logs/remotedesk-tv.log", "$Dir/logs/remotedesk-tv.log.1", "$Dir/diagnostics.txt")
foreach ($f in $files) {
  $name = Split-Path $f -Leaf
  if ($Route -eq 'Rooted') {
    scp "root@${TvHost}:$f" ".\$name" 2>$null
  } else {
    if (-not $KeyFile) { throw "-KeyFile is required for DevMode (see ares-setup-device --list)" }
    scp -P 9922 -i $KeyFile "prisoner@${TvHost}:$f" ".\$name" 2>$null
  }
  if (Test-Path ".\$name") { Write-Host "Copied $name" } else { Write-Host "Not present: $name" }
}

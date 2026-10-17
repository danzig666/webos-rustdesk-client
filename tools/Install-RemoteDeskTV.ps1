<#
.SYNOPSIS
  Install (or update) RemoteDesk TV on an LG webOS TV from Windows.

.DESCRIPTION
  -Route DevMode : uses LG's webOS CLI (ares-install / ares-launch) with a device registered via
                   ares-setup-device (Developer Mode, non-rooted TVs).
  -Route Rooted  : copies the IPK with scp and installs it through the TV's appInstallService over
                   SSH as root (rooted TVs with Homebrew Channel / SSH enabled).
  Nothing else on the TV is changed. Credentials are never stored by this script; SSH uses your
  existing key or prompts.

.EXAMPLE
  .\Install-RemoteDeskTV.ps1 -Ipk .\io.github.danzig666.remotedesktv_0.1.0_arm.ipk -Route DevMode -Device tv
.EXAMPLE
  .\Install-RemoteDeskTV.ps1 -Ipk .\io.github.danzig666.remotedesktv_0.1.0_arm.ipk -Route Rooted -TvHost 192.168.1.50
#>
param(
  [Parameter(Mandatory = $true)][string]$Ipk,
  [ValidateSet('DevMode', 'Rooted')][string]$Route = 'DevMode',
  [string]$Device = 'tv',
  [string]$TvHost,
  [switch]$NoLaunch
)
$ErrorActionPreference = 'Stop'
$AppId = 'io.github.danzig666.remotedesktv'
if (-not (Test-Path $Ipk)) { throw "IPK not found: $Ipk" }
Write-Host ("SHA-256: " + (Get-FileHash $Ipk -Algorithm SHA256).Hash)

if ($Route -eq 'DevMode') {
  if (-not (Get-Command ares-install -ErrorAction SilentlyContinue)) {
    throw "ares-install not found. Install LG's CLI: npm install -g @webos-tools/cli"
  }
  ares-install --device $Device $Ipk
  if (-not $NoLaunch) { ares-launch --device $Device $AppId }
} else {
  if (-not $TvHost) { throw "-TvHost is required for -Route Rooted" }
  scp $Ipk "root@${TvHost}:/tmp/remotedesk-tv.ipk"
  $payload = '{\"id\":\"com.ares.defaultName\",\"ipkUrl\":\"/tmp/remotedesk-tv.ipk\",\"subscribe\":true}'
  ssh "root@$TvHost" "luna-send -w 30000 -i luna://com.webos.appInstallService/dev/install '$payload' | grep -m1 -E 'installed|failed|error'; rm -f /tmp/remotedesk-tv.ipk"
  if (-not $NoLaunch) {
    ssh "root@$TvHost" "luna-send -n 1 luna://com.webos.applicationManager/launch '{\`"id\`":\`"$AppId\`"}'"
  }
}
Write-Host "Done."

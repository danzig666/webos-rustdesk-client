<#
.SYNOPSIS
  Run RemoteDesk TV's headless self test on a rooted TV over SSH (decoders, crypto, storage,
  NDL hardware decoder probe, advertised codecs).
#>
param([Parameter(Mandatory = $true)][string]$TvHost)
ssh "root@$TvHost" "/media/developer/apps/usr/palm/applications/io.github.danzig666.remotedesktv/bin/remotedesk-tv --self-test"

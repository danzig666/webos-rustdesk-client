# Install on a rooted TV (Homebrew Channel)

For TVs rooted with a supported method and running the
[webOS Homebrew Channel](https://github.com/webosbrew/webos-homebrew-channel). The app does **not**
need root to run. Root is only used here to install a package from a file.

> Test build: picture, keys, audio and hardware decoding have not been verified on a TV yet. Please
> report results (see [NONROOT_CHECKLIST.md](NONROOT_CHECKLIST.md), which also applies to rooted TVs).

## 1. Download

From the repository's **Releases** page download
`io.github.danzig666.remotedesktv_<version>_arm.ipk` and `SHA256SUMS.txt`, then check the hash:

```powershell
# Windows PowerShell
(Get-FileHash .\io.github.danzig666.remotedesktv_0.1.0_arm.ipk -Algorithm SHA256).Hash
```

The checksum only detects a damaged download. Trust comes from downloading over HTTPS from this
GitHub repository's release page.

## 2. Install

### Option A: webOS Dev Manager (Windows/macOS/Linux GUI)

1. Install [webOS Dev Manager](https://github.com/webosbrew/dev-manager-desktop).
2. Add the TV (rooted TVs are reachable as `root` over SSH, port 22).
3. **Apps → Install** and choose the `.ipk`.

### Option B: SSH (rooted TV)

```sh
scp io.github.danzig666.remotedesktv_0.1.0_arm.ipk root@<TV-IP>:/tmp/remotedesk-tv.ipk
ssh root@<TV-IP> 'luna-send -w 20000 -i luna://com.webos.appInstallService/dev/install \
  "{\"id\":\"com.ares.defaultName\",\"ipkUrl\":\"/tmp/remotedesk-tv.ipk\",\"subscribe\":true}"'
```

Wait until the reply contains `"state":"installed"`, then press Ctrl+C. The helper script
[tools/Install-RemoteDeskTV.ps1](../tools/Install-RemoteDeskTV.ps1) does the same from Windows.

### Option C: Homebrew Channel repository

Each release ships `io.github.danzig666.remotedesktv.manifest.json`, prepared for a Homebrew
repository. The app has **not** been submitted to the official webosbrew catalog. Until it is, use
option A or B.

## 3. Launch

Open **RemoteDesk TV** from the launcher. Always launch it through the launcher, not from an SSH
shell: hardware video only works for apps started by the TV's app manager.

## Update

Install the newer `.ipk` the same way. Settings, profiles and saved passwords are kept in the app's
`data/` directory. If an installer ever removes the app directory, they are lost; see
[SECURITY.md](SECURITY.md) for what is stored.

## Uninstall

* Launcher: highlight the app → remove, or
* `ssh root@<TV-IP> 'luna-send -w 20000 -i luna://com.webos.appInstallService/dev/remove "{\"id\":\"io.github.danzig666.remotedesktv\",\"subscribe\":true}"'`

## Logs and recovery

* Log file: `/media/developer/apps/usr/palm/applications/io.github.danzig666.remotedesktv/data/logs/remotedesk-tv.log`
  (the previous run is kept as `.log.1`). [tools/Get-RemoteDeskTVLogs.ps1](../tools/Get-RemoteDeskTVLogs.ps1) copies it.
* Headless check: `ssh root@<TV-IP> /media/developer/apps/usr/palm/applications/io.github.danzig666.remotedesktv/bin/remotedesk-tv --self-test`
* Settings → Diagnostics → **Save diagnostic report** writes `data/diagnostics.txt` (no passwords, tokens or typed text).
* If the app will not start, uninstall and reinstall. It never changes TV settings, root
  configuration, firmware or other apps.

# Install on a rooted TV (Homebrew Channel)

For TVs rooted with a supported method and running the
[webOS Homebrew Channel](https://github.com/webosbrew/webos-homebrew-channel). The app does **not**
need root to run. Root is only used here to install a package from a file.

## Recommended: the RustCouch catalog in the Homebrew Channel

The project publishes its own Homebrew Channel catalog with every release:

```
https://raw.githubusercontent.com/danzig666/webos-rustdesk-client/homebrew-repo/apps.json
```

1. Homebrew Channel → **Settings → Add repository** → enter the URL.
2. Back in the app list, open **RustCouch** → **Install**. The Homebrew Channel checks the
   package's SHA-256 from the catalog before installing.
3. To update, open the app's page in the Homebrew Channel and choose **Update**. RustCouch
   checks the catalog once per start and shows **Update … available** on Home, which opens the
   Homebrew Channel (Settings → "Check for updates" turns this off).

If you installed from an `.ipk` first, RustCouch → **Settings → Add the RustCouch catalog to
the Homebrew Channel** opens the Homebrew Channel with the URL already filled in.

## Other ways: install the `.ipk` file

Download `io.github.danzig666.remotedesktv_<version>_arm.ipk` from the repository's **Releases**
page (over HTTPS from this GitHub repository).

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
[tools/Install-RustCouch.ps1](../tools/Install-RustCouch.ps1) does the same from Windows.

## Launch

Open **RustCouch** from the launcher. Always launch it through the launcher, not from an SSH
shell: hardware video only works for apps started by the TV's app manager.

## Update

With the catalog: **Update** on the app's page in the Homebrew Channel. Otherwise install the newer
`.ipk` the same way. Settings, profiles and saved passwords are kept in the app's
`data/` directory. If an installer ever removes the app directory, they are lost; see
[SECURITY.md](SECURITY.md) for what is stored.

## Uninstall

* Launcher: highlight the app → remove, or
* `ssh root@<TV-IP> 'luna-send -w 20000 -i luna://com.webos.appInstallService/dev/remove "{\"id\":\"io.github.danzig666.remotedesktv\",\"subscribe\":true}"'`

## Logs and recovery

* Log file: `/media/developer/apps/usr/palm/applications/io.github.danzig666.remotedesktv/data/logs/remotedesk-tv.log`
  (the previous run is kept as `.log.1`). [tools/Get-RustCouchLogs.ps1](../tools/Get-RustCouchLogs.ps1) copies it.
* Headless check: `ssh root@<TV-IP> /media/developer/apps/usr/palm/applications/io.github.danzig666.remotedesktv/bin/remotedesk-tv --self-test`
* Settings → Diagnostics → **Save diagnostic report** writes `data/diagnostics.txt` (no passwords, tokens or typed text).
* If the app will not start, uninstall and reinstall. It never changes TV settings, root
  configuration, firmware or other apps.

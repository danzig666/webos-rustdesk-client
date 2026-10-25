# Install on a non-rooted TV (LG Developer Mode)

Baseline remote control must work without root. **This route has not been verified on a genuinely
non-rooted TV yet.** If you have one, please follow [NONROOT_CHECKLIST.md](NONROOT_CHECKLIST.md)
and report the results.

> If this TV runs the [Homebrew Channel](https://github.com/webosbrew/webos-homebrew-channel), the
> easiest install and update route is the project's catalog: see
> [README → Install and update with the Homebrew Channel](../README.md#install-and-update-with-the-homebrew-channel).
> The steps below use LG's own developer tools instead.

## Important about Developer Mode

* Developer Mode sessions expire (shown in the Developer Mode app) and must be renewed. When the
  session expires, **webOS removes sideloaded apps**. This is LG's lifecycle; this project does not
  bypass it, and a non-rooted install is not permanent.
* Do not enable Developer Mode on an already rooted TV just for this app; use
  [INSTALL_ROOTED.md](INSTALL_ROOTED.md) there.

## 1. Enable Developer Mode

Follow LG's guide: <https://webostv.developer.lge.com/develop/getting-started/developer-mode-app>
(LG developer account, install the **Developer Mode** app on the TV, turn on Dev Mode Status and
Key Server, reboot).

## 2. Install the tools

Either LG's CLI ([webOS TV CLI](https://webostv.developer.lge.com/develop/tools/cli-installation),
`npm install -g @webos-tools/cli`) or [webOS Dev Manager](https://github.com/webosbrew/dev-manager-desktop).

## 3. Register the TV (CLI)

```sh
ares-setup-device --add tv --info "{'host':'<TV-IP>','port':'9922','username':'prisoner'}"
ares-novacom --device tv --getkey     # enter the passphrase shown in the Developer Mode app
```

## 4. Install, launch, update, remove

```sh
ares-install --device tv io.github.danzig666.remotedesktv_0.1.0_arm.ipk
ares-launch  --device tv io.github.danzig666.remotedesktv
ares-install --device tv --list                                   # check it is installed
ares-install --device tv --remove io.github.danzig666.remotedesktv
```

Updating means installing the newer `.ipk`. From Windows,
[tools/Install-RustCouch.ps1](../tools/Install-RustCouch.ps1) wraps these commands
(`-Route DevMode`).

## Logs

The Developer Mode SSH key from `--getkey` lets you read the app's log:

```sh
ssh -i ~/.ssh/tv_webos -p 9922 prisoner@<TV-IP> \
  cat /media/developer/apps/usr/palm/applications/io.github.danzig666.remotedesktv/data/logs/remotedesk-tv.log
```

(`ares-setup-device --list` shows the key file name.) Settings → Diagnostics shows the same
information on screen.

## What to expect

* The app runs unprivileged; nothing in it needs root.
* Hardware video uses the TV's NDL DirectMedia library. Some TV generations restrict media devices
  for Developer Mode apps (reported by other projects for some k5lp/k3lp models). If that happens,
  Diagnostics shows the NDL error and the client falls back to software VP9/VP8 decoding.
  **Please report it**, with the Diagnostics report.

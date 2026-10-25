#!/usr/bin/env python3
"""Build a webOS IPK deterministically (no LG SDK needed).

Layout follows what webosbrew/ares-cli-rs `ares-package` produces: an `ar` archive with
`debian-binary`, `control.tar.gz` and `data.tar.gz`; data holds
usr/palm/applications/<id>/... and usr/palm/packages/<id>/packageinfo.json; tar entries
are owned by uid 0 / gid 5000 and directories are mode 0777.

Usage:
  package-ipk.py --binary B --sdl-lib L --version 0.1.0 --out dist [--app-id ID]
                 [--strip STRIP] [--title T]
"""
import argparse
import gzip
import hashlib
import io
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
EPOCH = int(os.environ.get("SOURCE_DATE_EPOCH", "1760000000"))


def gz(data: bytes) -> bytes:
    buf = io.BytesIO()
    with gzip.GzipFile(fileobj=buf, mode="wb", mtime=0, compresslevel=9) as g:
        g.write(data)
    return buf.getvalue()


def make_tar(root: str) -> bytes:
    """Deterministic tar of everything under `root` (sorted, fixed owner and mtime)."""
    buf = io.BytesIO()
    # Plain POSIX ustar, like ares-package: no GNU long-name or PAX extension records, which
    # minimal installers may not read. tarfile raises if a path no longer fits ustar.
    with tarfile.open(fileobj=buf, mode="w", format=tarfile.USTAR_FORMAT) as t:
        entries = []
        for dirpath, dirnames, filenames in os.walk(root):
            dirnames.sort()
            rel = os.path.relpath(dirpath, root)
            if rel != ".":
                entries.append(rel)
            for f in sorted(filenames):
                entries.append(os.path.normpath(os.path.join(rel, f)))
        for rel in sorted(entries):
            full = os.path.join(root, rel)
            ti = t.gettarinfo(full, arcname="./" + rel)
            ti.uid, ti.gid, ti.uname, ti.gname = 0, 5000, "root", "5000"
            ti.mtime = EPOCH
            if ti.isdir():
                ti.mode = 0o777
                t.addfile(ti)
            elif ti.issym():
                t.addfile(ti)
            else:
                ti.mode = 0o755 if os.access(full, os.X_OK) else 0o644
                with open(full, "rb") as fh:
                    t.addfile(ti, fh)
    return buf.getvalue()


def ar_archive(members) -> bytes:
    out = io.BytesIO()
    out.write(b"!<arch>\n")
    for name, data in members:
        hdr = "%-16s%-12d%-6d%-6d%-8s%-10d`\n" % (name, EPOCH, 0, 0, "100644", len(data))
        out.write(hdr.encode())
        out.write(data)
        if len(data) % 2:
            out.write(b"\n")
    return out.getvalue()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", required=True)
    ap.add_argument("--sdl-lib", required=True, help="path to libSDL2-2.0.so.0 (webOS backport)")
    ap.add_argument("--version", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--app-id", default=None)
    ap.add_argument("--title", default=None)
    ap.add_argument("--strip", default=None, help="strip tool for the target")
    ap.add_argument("--licenses", action="append", default=[], help="directory copied into licenses/ (repeatable)")
    a = ap.parse_args()

    parts = a.version.split(".")
    if len(parts) != 3 or not all(p.isdigit() for p in parts):
        sys.exit("webOS versions must be three numbers, e.g. 0.1.0")

    with open(os.path.join(ROOT, "packaging/webos/appinfo.json")) as f:
        appinfo = json.load(f)
    if a.app_id:
        appinfo["id"] = a.app_id
    if a.title:
        appinfo["title"] = a.title
    appinfo["version"] = a.version
    app_id = appinfo["id"]

    work = tempfile.mkdtemp(prefix="ipk-")
    try:
        data = os.path.join(work, "data")
        appdir = os.path.join(data, "usr/palm/applications", app_id)
        os.makedirs(os.path.join(appdir, "bin"))
        os.makedirs(os.path.join(appdir, "lib"))
        os.makedirs(os.path.join(appdir, "licenses"))
        binp = os.path.join(appdir, "bin", os.path.basename(appinfo["main"]))
        shutil.copyfile(a.binary, binp)
        os.chmod(binp, 0o755)
        sdl = os.path.join(appdir, "lib", "libSDL2-2.0.so.0")
        shutil.copyfile(os.path.realpath(a.sdl_lib), sdl)
        os.chmod(sdl, 0o755)
        if a.strip:
            subprocess.check_call([a.strip, "--strip-unneeded", binp])
            subprocess.check_call([a.strip, "--strip-unneeded", sdl])
        for src, dst in [
            ("packaging/webos/icon.png", "icon.png"),
            ("packaging/webos/icon_large.png", "icon_large.png"),
            ("LICENSE", "licenses/LICENSE-AGPL-3.0.txt"),
            ("THIRD-PARTY-NOTICES.txt", "licenses/THIRD-PARTY-NOTICES.txt"),
            ("crates/rdtv-app/assets/fonts/LICENSE-DejaVu.txt", "licenses/LICENSE-DejaVu.txt"),
            ("crates/rdtv-app/assets/fonts/LICENSE-Inter.txt", "licenses/LICENSE-Inter.txt"),
        ]:
            sp = os.path.join(ROOT, src)
            if os.path.exists(sp):
                shutil.copyfile(sp, os.path.join(appdir, dst))
        # Per-dependency licence files (hundreds, with long paths) are merged into one text
        # file so every path in the package stays short and the archive stays small.
        parts = []
        for ld in a.licenses:
            if not os.path.isdir(ld):
                continue
            for dirpath, dirnames, filenames in os.walk(ld):
                dirnames.sort()
                for fname in sorted(filenames):
                    fp = os.path.join(dirpath, fname)
                    rel = os.path.relpath(fp, ld)
                    with open(fp, "rb") as fh:
                        text = fh.read().decode("utf-8", "replace")
                    parts.append(f"{'=' * 78}\n{rel}\n{'=' * 78}\n\n{text.rstrip()}\n\n")
        if parts:
            with open(os.path.join(appdir, "licenses", "ALL-LICENSES.txt"), "w", encoding="utf-8") as f:
                f.write("Licence texts of every bundled component, one section per file.\n\n")
                f.write("".join(parts))
        with open(os.path.join(appdir, "appinfo.json"), "w") as f:
            json.dump(appinfo, f, indent=2)
            f.write("\n")
        pkgdir = os.path.join(data, "usr/palm/packages", app_id)
        os.makedirs(pkgdir)
        with open(os.path.join(pkgdir, "packageinfo.json"), "w") as f:
            json.dump({"id": app_id, "version": a.version, "app": app_id}, f, indent=2)
            f.write("\n")

        installed = 0
        for dp, _, fs in os.walk(data):
            for fn in fs:
                installed += os.path.getsize(os.path.join(dp, fn))
        control = (
            f"Package: {app_id}\nVersion: {a.version}\nSection: misc\nPriority: optional\n"
            f"Architecture: arm\nInstalled-Size: {installed}\n"
            f"Maintainer: RustCouch contributors <noreply@github.com>\n"
            f"Description: {appinfo['appDescription']}\n"
            f"webOS-Package-Format-Version: 2\nwebOS-Packager-Version: x.y.x\n"
        )
        ctl = os.path.join(work, "control")
        os.makedirs(ctl)
        with open(os.path.join(ctl, "control"), "w") as f:
            f.write(control)
        ipk = ar_archive([
            ("debian-binary", b"2.0\n"),
            ("control.tar.gz", gz(make_tar(ctl))),
            ("data.tar.gz", gz(make_tar(data))),
        ])
        os.makedirs(a.out, exist_ok=True)
        name = f"{app_id}_{a.version}_arm.ipk"
        path = os.path.join(a.out, name)
        with open(path, "wb") as f:
            f.write(ipk)
        digest = hashlib.sha256(ipk).hexdigest()
        print(f"{path} {len(ipk)} bytes sha256 {digest}")
    finally:
        shutil.rmtree(work)


if __name__ == "__main__":
    main()

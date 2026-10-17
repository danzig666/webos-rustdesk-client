#!/usr/bin/env python3
"""Generate THIRD-PARTY-NOTICES.txt and a CycloneDX 1.5 SBOM (sbom.cdx.json) for the
TV build from `cargo metadata` plus the bundled/static C components.

Usage: gen-notices.py [--target armv7-unknown-linux-gnueabi] [--out-dir .]
"""
import argparse
import json
import os
import subprocess

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))

C_COMPONENTS = [
    {"name": "SDL2 (webosbrew SDL-webOS backport)", "version": "2.30.12-webos.6", "license": "Zlib",
     "url": "https://github.com/webosbrew/SDL-webOS", "how": "shared library bundled in lib/"},
    {"name": "libvpx", "version": "1.15.2 (d168454e)", "license": "BSD-3-Clause",
     "url": "https://chromium.googlesource.com/webm/libvpx", "how": "statically linked (VP8/VP9 software decoding)"},
    {"name": "libopus", "version": "1.5.2", "license": "BSD-3-Clause",
     "url": "https://opus-codec.org", "how": "statically linked (audio decoding)"},
    {"name": "OpenSSL (via openssl-src)", "version": "3.x (vendored)", "license": "Apache-2.0",
     "url": "https://www.openssl.org", "how": "statically linked (TLS used by upstream hbb_common)"},
    {"name": "libsodium (via libsodium-sys)", "version": "1.0.x (bundled)", "license": "ISC",
     "url": "https://libsodium.org", "how": "statically linked (RustDesk key exchange)"},
    {"name": "DejaVu fonts", "version": "2.37", "license": "Bitstream-Vera AND LicenseRef-Public-Domain",
     "url": "https://dejavu-fonts.github.io", "how": "embedded in the executable"},
]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--target", default="armv7-unknown-linux-gnueabi")
    ap.add_argument("--out-dir", default=ROOT)
    ap.add_argument("--licenses-dir", default=None, help="copy every crate's LICENSE/COPYING/NOTICE files here")
    a = ap.parse_args()
    meta = json.loads(subprocess.check_output(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", a.target],
        cwd=ROOT))
    # Only packages reachable from the app's resolve graph.
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    pkgs = {p["id"]: p for p in meta["packages"]}
    root = next(p["id"] for p in meta["packages"] if p["name"] == "rdtv-app")
    seen, stack = set(), [root]
    while stack:
        i = stack.pop()
        if i in seen:
            continue
        seen.add(i)
        for d in nodes[i]["deps"]:
            normal = any(k.get("kind") in (None, "normal") for k in d["dep_kinds"])
            # `*-src` build dependencies (e.g. openssl-src) compile C code into the binary.
            vendored = pkgs[d["pkg"]]["name"].endswith("-src")
            if normal or vendored:
                stack.append(d["pkg"])
    crates = sorted((pkgs[i] for i in seen), key=lambda p: (p["name"], p["version"]))

    lines = [
        "RemoteDesk TV - third-party notices",
        "=" * 36,
        "",
        "RemoteDesk TV is licensed under the GNU AGPL-3.0 (see LICENSE-AGPL-3.0.txt).",
        "Complete corresponding source: https://github.com/danzig666/webos-rustdesk-client",
        "(including the pinned RustDesk submodule and scripts/build-deps.sh for the C libraries).",
        "",
        "Native components",
        "-----------------",
    ]
    for c in C_COMPONENTS:
        lines.append(f"* {c['name']} {c['version']} - {c['license']} - {c['url']} ({c['how']})")
    lines += ["", "Rust crates (name version - license - source)", "-" * 45]
    for p in crates:
        src = p.get("repository") or (p.get("source") or "workspace")
        lines.append(f"* {p['name']} {p['version']} - {p.get('license') or 'see source'} - {src}")
    lines += ["", "The full license texts of these components are available from their sources",
              "listed above; texts required to accompany binaries are included in licenses/."]
    with open(os.path.join(a.out_dir, "THIRD-PARTY-NOTICES.txt"), "w") as f:
        f.write("\n".join(lines) + "\n")

    if a.licenses_dir:
        import shutil
        n = 0
        for p in crates:
            d = os.path.dirname(p["manifest_path"])
            if p["name"] in ("rdtv-app", "rdtv-core", "rdtv-media"):
                continue
            dest = os.path.join(a.licenses_dir, "crates", f"{p['name']}-{p['version']}")
            for fn in sorted(os.listdir(d)):
                up = fn.upper()
                if up.startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT")) and os.path.isfile(os.path.join(d, fn)):
                    os.makedirs(dest, exist_ok=True)
                    shutil.copyfile(os.path.join(d, fn), os.path.join(dest, fn))
                    n += 1
            # Bundled C sources inside -sys / -src crates.
            for sub in ("libsodium", "openssl", "src/libsodium"):
                sd = os.path.join(d, sub)
                if os.path.isdir(sd):
                    for fn in os.listdir(sd):
                        if fn.upper().startswith(("LICENSE", "COPYING")) and os.path.isfile(os.path.join(sd, fn)):
                            os.makedirs(dest, exist_ok=True)
                            shutil.copyfile(os.path.join(sd, fn), os.path.join(dest, sub.replace("/", "_") + "-" + fn))
                            n += 1
        print(f"copied {n} license files")
    comps = []
    for p in crates:
        c = {"type": "library", "name": p["name"], "version": p["version"],
             "purl": f"pkg:cargo/{p['name']}@{p['version']}"}
        if p.get("license"):
            c["licenses"] = [{"expression": p["license"]}]
        comps.append(c)
    for c in C_COMPONENTS:
        comps.append({"type": "library", "name": c["name"], "version": c["version"],
                      "licenses": [{"expression": c["license"]}],
                      "externalReferences": [{"type": "website", "url": c["url"]}]})
    sbom = {"bomFormat": "CycloneDX", "specVersion": "1.5", "version": 1,
            "metadata": {"component": {"type": "application", "name": "remotedesk-tv",
                                       "version": next(p["version"] for p in meta["packages"] if p["name"] == "rdtv-app")}},
            "components": comps}
    with open(os.path.join(a.out_dir, "sbom.cdx.json"), "w") as f:
        json.dump(sbom, f, indent=1)
    print(f"{len(crates)} crates, {len(C_COMPONENTS)} native components")


if __name__ == "__main__":
    main()

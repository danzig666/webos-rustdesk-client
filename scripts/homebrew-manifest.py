#!/usr/bin/env python3
"""Write a webOS Homebrew Channel repository manifest for a built IPK
(format used by repo.webosbrew.org: id, version, type, title, iconUri, sourceUrl,
rootRequired, ipkUrl, ipkHash). scripts/homebrew-repo.py publishes it in the project's own
catalog (the homebrew-repo branch)."""
import argparse
import hashlib
import json

REPO = "https://github.com/danzig666/webos-rustdesk-client"
CATALOG = "https://raw.githubusercontent.com/danzig666/webos-rustdesk-client/homebrew-repo"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--ipk", required=True)
    ap.add_argument("--version", required=True)
    ap.add_argument("--tag", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    with open(a.ipk, "rb") as f:
        digest = hashlib.sha256(f.read()).hexdigest()
    name = a.ipk.split("/")[-1]
    manifest = {
        "id": "io.github.danzig666.remotedesktv",
        "version": a.version,
        "type": "native",
        "title": "RustCouch",
        "appDescription": "RustDesk-compatible remote desktop client (community project)",
        "iconUri": f"{CATALOG}/icon.png",
        "sourceUrl": REPO,
        "rootRequired": False,
        "ipkUrl": f"{REPO}/releases/download/{a.tag}/{name}",
        "ipkHash": {"sha256": digest},
    }
    with open(a.out, "w") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")
    print(json.dumps(manifest, indent=2))


if __name__ == "__main__":
    main()

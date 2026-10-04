#!/usr/bin/env python3
"""Write the Tauri updater manifest (latest.json) for one release (TUR-66).

release.yml's `publish` job runs this on the folder of release files. Each
updater artifact must have its `.sig` (from `tauri signer sign`) beside it.
The app fetches the result from
https://github.com/<repo>/releases/latest/download/latest.json
(tauri.conf.json `plugins.updater.endpoints`).

Keys are `OS-ARCH` as tauri-plugin-updater 2.x reads them. It looks up
`<os>-<arch>-<installer>` first and then `<os>-<arch>`, so Windows and Linux
get both: the plain key names the NSIS installer and the AppImage, the only
update paths we ship (the .deb is never an update target).

Usage:
  updater-json.py --version 0.4.0 --tag v0.4.0 --repo owner/name \
      --dir release-out [--notes-file notes.md] [--out latest.json]
"""

import argparse
import datetime
import json
import pathlib
import sys

# (platform keys, file name after "meet-ai-<version>-")
ARTIFACTS = [
    (["darwin-aarch64", "darwin-aarch64-app"], "macos-arm64.app.tar.gz"),
    (["windows-x86_64", "windows-x86_64-nsis"], "windows-x64-setup.exe"),
    (["linux-x86_64", "linux-x86_64-appimage"], "linux-x86_64.AppImage"),
]


def build(version, tag, repo, folder, notes="", now=None):
    """Return the manifest dict, or raise ValueError naming what is missing."""
    folder = pathlib.Path(folder)
    now = now or datetime.datetime.now(datetime.timezone.utc)
    platforms = {}
    missing = []
    for keys, suffix in ARTIFACTS:
        name = f"meet-ai-{version}-{suffix}"
        sig = folder / f"{name}.sig"
        if not (folder / name).is_file():
            missing.append(name)
            continue
        if not sig.is_file():
            missing.append(sig.name)
            continue
        signature = sig.read_text(encoding="utf-8").strip()
        if not signature:
            missing.append(f"{sig.name} (empty)")
            continue
        entry = {
            "signature": signature,
            "url": f"https://github.com/{repo}/releases/download/{tag}/{name}",
        }
        for key in keys:
            platforms[key] = dict(entry)
    if missing:
        raise ValueError("missing: " + ", ".join(missing))
    return {
        "version": version,
        "notes": notes,
        # RFC 3339, as the updater parses it.
        "pub_date": now.replace(microsecond=0).isoformat().replace("+00:00", "Z"),
        "platforms": platforms,
    }


def main(argv):
    p = argparse.ArgumentParser()
    p.add_argument("--version", required=True)
    p.add_argument("--tag", required=True)
    p.add_argument("--repo", required=True)
    p.add_argument("--dir", required=True)
    p.add_argument("--notes-file")
    p.add_argument("--out", default="latest.json")
    a = p.parse_args(argv)
    notes = ""
    if a.notes_file:
        notes = pathlib.Path(a.notes_file).read_text(encoding="utf-8").strip()
    try:
        manifest = build(a.version, a.tag, a.repo, a.dir, notes)
    except ValueError as e:
        print(f"::error::latest.json: {e}", file=sys.stderr)
        return 1
    pathlib.Path(a.out).write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"wrote {a.out}: {', '.join(sorted(manifest['platforms']))}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

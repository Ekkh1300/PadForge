"""Create the v1.0.1 release.

Separate from update_release.py on purpose: that one replaces the binary on an
existing tag, and this one creates a tag, a release, and every asset. Merging
them would mean one script with two jobs and a flag saying which, and the flag
would be wrong half the time.

The notes are English here, as asked. docs/release-notes.md stays Persian for
the v1.0.0 tag, which is already published and must not change under anyone who
downloaded it.

Run:  python tools/make_release.py
"""

import json
import pathlib
import subprocess
import sys

REPO = "Ekkh1300/PadForge"
TAG = "v1.0.1"
TITLE = "PadForge 1.0.1"
ROOT = pathlib.Path(r"D:\ds op\PadForge")
SETUP = ROOT / "dist" / "PadForge-Setup.exe"
NOTES = ROOT / "docs" / "release-notes-en.md"

# (source, uploaded-as). The names are what the release page links to, and the
# notes reference them, so they are spelled out once here rather than in two
# places that can disagree.
ASSETS = [
    (ROOT / "dist" / "PadForge-Setup.exe", "PadForge-Setup.exe"),
    (ROOT / "docs" / "installer.png", "installer.png"),
    (ROOT / "docs" / "icon-128.png", "icon-128.png"),
    (ROOT / "docs" / "icon-512.png", "icon-512.png"),
    (ROOT / "docs" / "icon-sizes.png", "icon-sizes.png"),
]


def gh(*args):
    return subprocess.run(
        ["gh", *args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
    )


def main() -> int:
    if not SETUP.exists():
        print(f"missing {SETUP}", file=sys.stderr)
        return 1
    for src, _ in ASSETS:
        if not src.exists():
            print(f"missing asset {src}", file=sys.stderr)
            return 1

    existing = gh("release", "view", TAG, "--repo", REPO, "--json", "tagName")
    if existing.returncode == 0:
        print(f"{TAG} already exists; use update_release.py to replace its binary")
        return 1

    print(f"creating {TAG}")
    created = gh(
        "release", "create", TAG,
        "--repo", REPO,
        "--title", TITLE,
        "--notes-file", str(NOTES),
        # Not a prerelease and not a draft: this is the release people install.
        "--verify-tag" if False else "--target", "main",
    )
    if created.returncode != 0:
        print(created.stderr, file=sys.stderr)
        return 1

    for src, name in ASSETS:
        print(f"uploading {name} ({src.stat().st_size:,} bytes)")
        up = gh(
            "release", "upload", TAG, str(src),
            "--repo", REPO, "--clobber",
        )
        if up.returncode != 0:
            print(up.stderr, file=sys.stderr)
            return 1

    check = gh(
        "release", "view", TAG, "--repo", REPO,
        "--json", "assets,tagName,url,isDraft,isPrerelease",
    )
    if check.returncode != 0:
        print(check.stderr, file=sys.stderr)
        return 1
    info = json.loads(check.stdout)
    print(f"\n{info['url']}")
    print(f"draft={info['isDraft']}  prerelease={info['isPrerelease']}")
    for asset in info.get("assets", []):
        print(f"  {asset['name']:<22} {asset['size']:>10,} bytes")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
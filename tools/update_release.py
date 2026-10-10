"""Update the v1.0.0 release: replace the setup binary and rewrite the notes.

Run through Python rather than PowerShell because the notes are Persian and
`gh --template` piped through PowerShell's redirection re-encodes them. Reading
and writing the body as explicit UTF-8 keeps the characters intact.
"""

import json
import pathlib
import subprocess
import sys

REPO = "Ekkh1300/PadForge"
TAG = "v1.0.0"
ROOT = pathlib.Path(r"D:\ds op\PadForge")
SETUP = ROOT / "dist" / "PadForge-Setup.exe"
NOTES = ROOT / "docs" / "release-notes.md"


def gh(*args, **kwargs):
    return subprocess.run(
        ["gh", *args],
        capture_output=True,
        text=True,
        encoding="utf-8",
        errors="replace",
        **kwargs,
    )


def main() -> int:
    body = NOTES.read_text(encoding="utf-8")

    if not SETUP.exists():
        print(f"missing {SETUP}", file=sys.stderr)
        return 1

    current = gh(
        "release", "view", TAG, "--repo", REPO,
        "--json", "assets,tagName",
    )
    if current.returncode != 0:
        print(current.stderr, file=sys.stderr)
        return 1
    release = json.loads(current.stdout)

    # Replace, do not pile up: a second upload would leave the old binary
    # attached under the same name and the link would keep serving it.
    for asset in release.get("assets", []):
        if asset["name"] == SETUP.name:
            print(f"deleting old asset {asset['name']} ({asset['size']} bytes)")
            deleted = gh(
                "release", "delete-asset", TAG, asset["name"],
                "--repo", REPO, "--yes",
            )
            if deleted.returncode != 0:
                print(deleted.stderr, file=sys.stderr)
                return 1

    print(f"uploading {SETUP.name} ({SETUP.stat().st_size} bytes)")
    uploaded = gh(
        "release", "upload", TAG, str(SETUP),
        "--repo", REPO, "--clobber",
    )
    if uploaded.returncode != 0:
        print(uploaded.stderr, file=sys.stderr)
        return 1

    print("rewriting the release notes")
    edited = gh(
        "release", "edit", TAG, "--repo", REPO,
        "--title", "PadForge 1.0.0",
        "--notes-file", str(NOTES),
    )
    if edited.returncode != 0:
        print(edited.stderr, file=sys.stderr)
        return 1

    check = gh(
        "release", "view", TAG, "--repo", REPO,
        "--json", "assets,tagName,url",
    )
    info = json.loads(check.stdout)
    print(f"\n{info['url']}")
    for asset in info.get("assets", []):
        print(f"  {asset['name']:<22} {asset['size']:>10,} bytes")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

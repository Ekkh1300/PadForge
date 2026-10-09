"""Reports what the published release actually contains.

Runs `gh` itself rather than parsing whatever PowerShell piped through, because
the JSON lands in a different encoding depending on the shell that collected it,
and a failed parse looks identical to a failed query.
"""
import json
import subprocess
import sys

REPO = "Ekkh1300/PadForge"


def gh(*args):
    out = subprocess.run(
        ["gh", *args], capture_output=True, text=True, encoding="utf-8", errors="replace"
    )
    if out.returncode != 0:
        print(f"  gh {' '.join(args)} failed:\n{out.stderr.strip()}")
        return None
    return out.stdout


def main():
    tag = sys.argv[1] if len(sys.argv) > 1 else "v1.0.0"

    raw = gh("release", "view", tag, "--repo", REPO,
             "--json", "name,tagName,isDraft,url,assets,publishedAt,body")
    if raw is None:
        return 1

    d = json.loads(raw)
    print(f"  title     : {d['name']}")
    print(f"  url       : {d['url']}")
    print(f"  draft     : {d['isDraft']}")
    print(f"  published : {d.get('publishedAt')}")

    body = d.get("body") or ""
    print(f"  notes     : {len(body)} characters")
    # A release page that renders as a wall of raw Persian with no image is the
    # failure this is here to catch, so the image references are checked.
    for needle in ("icon-512", "installer.png", "ViGEmBus"):
        print(f"    mentions {needle:<16} {needle in body}")

    print("\n  assets:")
    total = 0
    for a in d["assets"]:
        size = a["size"]
        total += size
        print(f"    {a['name']:<26} {size/1024:>8.0f} KB  {a['state']}")
    print(f"    {'total':<26} {total/1024/1024:>8.1f} MB")

    # The setup file is the whole point of the release; if it is missing or is a
    # zero-byte placeholder from an earlier empty build, say so.
    setup = [a for a in d["assets"] if a["name"].endswith(".exe")]
    if not setup:
        print("\n  PROBLEM: no executable in the release")
        return 1
    if setup[0]["size"] < 1_000_000:
        print(f"\n  PROBLEM: {setup[0]['name']} is only {setup[0]['size']} bytes, "
              "which means an empty build was uploaded")
        return 1

    print(f"\n  {setup[0]['name']} is {setup[0]['size']/1024/1024:.1f} MB, which is a real binary")
    return 0


if __name__ == "__main__":
    sys.exit(main())
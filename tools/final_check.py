"""Final check on the published repository.

Everything here is a question that has an objective answer and can be asked
against the live site: does the release exist, is the binary the right size, does
CI pass, is anything uncommitted locally, and does the tree still build clean.
"""
import json
import os
import subprocess
import sys

ROOT = r"D:\ds op\PadForge"
REPO = "Ekkh1300/PadForge"
SETUP = os.path.join(ROOT, "dist", "PadForge-Setup.exe")

failures = []
notes = []


def gh(*args):
    out = subprocess.run(
        ["gh", *args], capture_output=True, text=True, encoding="utf-8", errors="replace"
    )
    return out.stdout if out.returncode == 0 else None


def check(label, ok, detail=""):
    mark = "ok  " if ok else "FAIL"
    print(f"  {mark} {label:<42} {detail}")
    if not ok:
        failures.append(label)


def run(*args, cwd=ROOT):
    return subprocess.run(args, cwd=cwd, capture_output=True, text=True, encoding="utf-8", errors="replace")


print("the release")
rel = gh("release", "view", "v1.0.0", "--repo", REPO, "--json", "name,isDraft,url,assets,body")
if rel is None:
    check("release v1.0.0 exists", False)
else:
    d = json.loads(rel)
    check("release is published", not d["isDraft"], d["url"])
    body = d.get("body") or ""
    for name, needle in (
        ("release notes show the icon", "icon-512.png"),
        ("release notes show the installer", "installer.png"),
        ("release notes link the driver", "ViGEmBus"),
        ("release notes are Persian", "بازی‌های ویندوز"),
    ):
        check(name, needle in body)

    assets = {a["name"]: a for a in d["assets"]}
    check("setup file is attached", "PadForge-Setup.exe" in assets)
    if "PadForge-Setup.exe" in assets:
        size = assets["PadForge-Setup.exe"]["size"]
        # A real build with an embedded application is several megabytes. An empty
        # payload would be a few hundred kilobytes at most.
        check("setup file is a real binary", size > 5_000_000, f"{size/1024/1024:.1f} MB")
    for name in ("icon-128.png", "icon-512.png", "icon-sizes.png", "installer.png"):
        check(f"{name} attached", name in assets)

print("\nthe local artefact matches what was published")
if os.path.isfile(SETUP):
    local = os.path.getsize(SETUP)
    remote = assets.get("PadForge-Setup.exe", {}).get("size")
    check("local size equals published size", local == remote, f"{local:,} bytes")
else:
    check("local setup file exists", False)

print("\nthe repository")
repo = gh("repo", "view", REPO, "--json", "visibility,defaultBranchRef,repositoryTopics,description")
if repo:
    r = json.loads(repo)
    check("repository is public", r["visibility"] == "PUBLIC")
    check("default branch is main", r["defaultBranchRef"]["name"] == "main")
    topics = [t["name"] for t in (r.get("repositoryTopics") or [])]
    check("has topics", len(topics) >= 5, ", ".join(topics))

runs = gh("run", "list", "--repo", REPO, "--limit", "1", "--json", "status,conclusion,name")
if runs:
    r = json.loads(runs)[0]
    check("latest CI run is green", r["conclusion"] == "success", r["name"])

print("\nthe working tree")
st = run("git", "status", "--porcelain")
check("nothing uncommitted", not st.stdout.strip(), st.stdout.strip()[:60])
local_sha = run("git", "rev-parse", "HEAD").stdout.strip()
remote_sha = run("git", "ls-remote", "origin", "main").stdout.split()[0]
check("local matches origin", local_sha == remote_sha, local_sha[:8])

print("\nthe build")
fmt = run("cargo", "fmt", "--all", "--check")
check("cargo fmt --check", fmt.returncode == 0)
clippy = run(
    "cargo", "clippy", "--workspace", "--all-targets", "--target", "x86_64-pc-windows-gnu"
)
noise = [l for l in clippy.stderr.splitlines() if l.startswith(("warning:", "error:")) and "generated" not in l]
check("clippy is clean", not noise, noise[0] if noise else "")
doc = run("cargo", "doc", "--workspace", "--no-deps", "--target", "x86_64-pc-windows-gnu")
doc_warn = [l for l in doc.stderr.splitlines() if l.startswith("warning")]
check("rustdoc is clean", not doc_warn, doc_warn[0] if doc_warn else "")

print("\nthe icon")
py = sys.executable
before = run("python", "-c", "import hashlib;print(hashlib.sha256(open(r'tools/padforge.ico','rb').read()).hexdigest())")
gen = run(py, os.path.join(ROOT, "tools", "make_icon.py"))
after = run("python", "-c", "import hashlib;print(hashlib.sha256(open(r'tools/padforge.ico','rb').read()).hexdigest())")
check("icon regenerates identically", before.stdout.strip() == after.stdout.strip())
check("icon generator needs no Pillow", "PIL" not in open(os.path.join(ROOT, "tools", "make_icon.py"), encoding="utf-8").read())

print()
if failures:
    print(f"{len(failures)} check(s) failed: {', '.join(failures)}")
    sys.exit(1)
print("everything checks out")
sys.exit(0)
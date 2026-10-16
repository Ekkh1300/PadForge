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
# The tag is not pinned. It was v1.0.0 through the first release and had to be
# edited by hand each time after, which is the kind of thing that silently stops
# checking the thing you think it is checking. Read it from the workspace
# version instead: that is the release being built, by construction.
cargo_toml = open(os.path.join(ROOT, "Cargo.toml"), encoding="utf-8").read()
VERSION = cargo_toml.split('version = "')[1].split('"')[0]
TAG = f"v{VERSION}"
rel = gh("release", "view", TAG, "--repo", REPO, "--json", "name,isDraft,url,assets,body")
if rel is None:
    check(f"release {TAG} exists", False)
else:
    d = json.loads(rel)
    check("release is published", not d["isDraft"], d["url"])
    body = d.get("body") or ""
    for name, needle in (
        ("release notes show the icon", "icon-512.png"),
        ("release notes show the installer", "installer.png"),
        ("release notes link the driver", "ViGEmBus"),
        # The property, not a sentence from one particular release. This used to
        # look for "Windows games speak XInput", which is a line that happened to
        # be in v1.0.1 -- so every later release failed a check about being in
        # English. The notes are English as asked, so the invariant worth
        # checking is that they are not Persian: Arabic-script characters, which
        # are what a Persian document is made of and an English one has none of.
        ("release notes are in English", not any("\u0600" <= c <= "\u06ff" for c in body)),
    ):
        if isinstance(needle, bool):
            check(name, needle)
        else:
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

print("\nwhat was published can be verified")
# This used to compare the size of the locally built installer against the
# published one, which only meant anything while releases were built by hand on
# this machine. Now a tag builds them on a runner, and two correct builds of the
# same source are not the same bytes -- so the check was comparing a stale local
# artefact against a good remote one, and failing for a reason that said nothing
# about either.
#
# The property worth checking is the one a downloader can rely on: the checksum
# sitting next to the installer is the checksum of the installer. GitHub computes
# a digest per asset, and the release job computed the other one from the same
# file in the same run. If those two agree, somebody who downloads both can prove
# they match.
sums_asset = assets.get("SHA256SUMS.txt")
setup_asset = assets.get("PadForge-Setup.exe")
if sums_asset and setup_asset:
    # The REST endpoint, not the GraphQL one `gh release view` uses: the
    # GraphQL asset `id` is a node id ("RA_kwDO..."), not the number
    # `/releases/assets/{id}` wants, so the obvious version of this returns
    # Not Found and then compares the string "Not Found" against a hash.
    rest = run("gh", "api", f"repos/{REPO}/releases/tags/{TAG}")
    rest_assets = {}
    if rest.returncode == 0:
        rest_assets = {a["name"]: a for a in json.loads(rest.stdout).get("assets", [])}

    sums = run(
        "gh", "api",
        f"repos/{REPO}/releases/assets/{rest_assets['SHA256SUMS.txt']['id']}",
        "-H", "Accept: application/octet-stream",
    ).stdout.strip()
    parts = sums.split()
    published_hash = parts[0] if parts else ""
    github_hash = (rest_assets["PadForge-Setup.exe"].get("digest") or "").removeprefix("sha256:")
    check(
        "the shipped checksum is the checksum of the shipped installer",
        bool(published_hash) and published_hash == github_hash,
        published_hash[:16] or sums[:40],
    )
    # `<hash>  <name>`, two spaces: one space does not parse for sha256sum -c.
    check("the checksum file is in sha256sum format", "  " in sums, sums[:60])
else:
    check("a checksum ships with the installer", False)

if os.path.isfile(SETUP):
    local = os.path.getsize(SETUP)
    print(f"  note: local installer is {local:,} bytes; the release was built on a runner")
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

print("\nthe project's own files")
# A public repo whose contribution guide and issue templates are missing is a
# repo that tells nobody how to help it, which is a way of saying no.
for name in (
    "CONTRIBUTING.md",
    "LICENSE",
    ".github/ISSUE_TEMPLATE/bug_report.yml",
    ".github/ISSUE_TEMPLATE/feature_request.yml",
    "docs/roadmap.md",
    "tools/release_notes.py",
):
    path = os.path.join(ROOT, *name.split("/"))
    check(f"{name} is committed", os.path.isfile(path) and not run("git", "ls-files", "--error-unmatch", name).returncode)

repo_full = gh("api", f"repos/{REPO}", "--jq", ".has_discussions")
check("discussions are on", repo_full.strip() == "true")

# A tag-triggered build is the difference between the release page and a
# download a person compiled themselves. Checked in the workflow rather than
# assumed, because a trigger typed slightly wrong looks identical to one that
# was never added.
workflow = open(os.path.join(ROOT, ".github", "workflows", "ci.yml"), encoding="utf-8").read()
check("a tag triggers a build", 'tags: ["v*"]' in workflow)
check("a release job exists", "\n  release:" in workflow)
check("releases carry a checksum", "SHA256SUMS" in workflow)

print("\nthe hot path")
perf = run(
    "cargo", "test", "-p", "padcore", "--release", "--target", "x86_64-pc-windows-gnu",
    "--test", "perf", "--", "--ignored", "--nocapture", "--test-threads=1",
)
check("the hot path is within budget", perf.returncode == 0)

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

print("\nthe changelog generator")
notes = run(py, os.path.join(ROOT, "tools", "test_release_notes.py"))
check("release-notes tests pass", notes.returncode == 0, notes.stderr.strip().splitlines()[-1] if notes.stderr.strip() else "")

print()
if failures:
    print(f"{len(failures)} check(s) failed: {', '.join(failures)}")
    sys.exit(1)
print("everything checks out")
sys.exit(0)
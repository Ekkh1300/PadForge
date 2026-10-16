"""Build release notes from conventional commits.

The subject is that the notes stop being written by hand. A hand-written
changelog is either forgotten or, when it is not, wrong: the two bugs fixed last
week are not in it, because writing them is a separate task from fixing them and
the separate task always loses. Here the two are the same commit.

Input is the commit log between two tags, parsed for a conventional-commit
subject. The output is grouped by the type, which is why the format matters:
`fix` lands under a heading that says fixes, `feat` under features, and a commit
with no type is left out of the summary rather than being invented into one.

Run:
    python tools/release_notes.py v1.0.0 v1.0.1
    python tools/release_notes.py v1.0.1        # everything since that tag

Output goes to stdout as Markdown, ready for `--notes-file`.

Standard library only, like the icon generator: this runs in CI.
"""

from __future__ import annotations

import re
import subprocess
import sys

# `type(scope)!: subject`, the conventional-commit shape. The type is what
# decides the group, so it is captured separately from the rest.
COMMIT = re.compile(
    r"""^
    (?P<type>[a-zA-Z]+)      # fix, feat, perf, docs, refactor, ...
    (\((?P<scope>[^)]*)\))?  # optional area in brackets
    (?P<breaking>!)?         # the exclamation mark
    :\s+
    (?P<subject>.+)
    $""",
    re.VERBOSE,
)

# Ordered, and the order is the order of interest to someone deciding whether to
# update: what broke and got fixed, then what is new, then what got faster.
GROUPS: list[tuple[str, str, list[str]]] = [
    ("fix", "Fixes", []),
    ("feat", "New", []),
    ("perf", "Performance", []),
    ("refactor", "Changed", []),
    ("docs", "Documentation", []),
    ("test", "Tests", []),
    ("build", "Build", []),
    ("ci", "Build", []),
]

# Human wording for each type, so the heading reads like English rather than
# like a git convention. "Fixes" on its own is not a thing anyone says.
HEADINGS = {
    "fix": "Fixes",
    "feat": "New",
    "perf": "Performance",
    "refactor": "Changed",
    "docs": "Documentation",
    "test": "Tests",
    "build": "Build",
    "ci": "Build",
}

# Types that describe the work rather than the user. A "chore: bump the
# dependency" is real and belongs in the log, and does not belong in a list of
# what changed for someone deciding whether to install this.
INTERNAL = {"chore", "style", "ci"}


def git(*args: str) -> str:
    result = subprocess.run(
        ["git", *args], capture_output=True, text=True, encoding="utf-8", errors="replace"
    )
    if result.returncode != 0:
        print(f"git {' '.join(args)} failed: {result.stderr.strip()}", file=sys.stderr)
        raise SystemExit(1)
    return result.stdout


def parse_subject(subject: str) -> tuple[str, str, str, bool] | None:
    """(type, scope, subject, breaking), or None if there is nothing to say.

    A subject that does not follow the convention still comes back, under
    ``other``. That is the important part: a generator that silently drops
    commits it does not recognise produces a confident, complete-looking
    changelog that is missing the one bug a user cared about. Nobody notices
    the absence. So they are kept, and the caller is told how many there were.
    """
    line = subject.splitlines()[0].strip() if subject else ""
    if not line:
        return None

    match = COMMIT.match(line)
    if not match:
        return ("other", "", sentence(line), False)

    kind = match.group("type").lower()
    if kind not in {g[0] for g in GROUPS} | INTERNAL:
        return ("other", "", sentence(line), False)

    scope = match.group("scope") or ""
    return (kind, scope, sentence(match.group("subject")), bool(match.group("breaking")))


def sentence(text: str) -> str:
    """A commit subject as a sentence.

    Commit subjects are capitalised and unpunctuated, because that is how commit
    subjects are written. Prose wants the opposite.
    """
    text = text.strip()
    if not text:
        return text
    text = text[0].upper() + text[1:]
    if not text.endswith((".", "!", "?")):
        text += "."
    return text


def collect(since: str | None) -> list[tuple[str, str, str, bool]]:
    rng = f"{since}..HEAD" if since else "HEAD"
    # A stable, NUL-separated log. `-z` because a subject can contain anything
    # else, including a newline once %B is involved -- not here, since %s is one
    # line by definition, but the separator still avoids quoting problems with
    # quotes and backslashes in a subject, which are ordinary.
    raw = git("log", "--no-merges", "--format=%s%x00", rng)
    subjects = [s for s in raw.split("\0") if s.strip()]
    return [p for p in (parse_subject(s) for s in subjects) if p]


def render(commits: list[tuple[str, str, str, bool]]) -> str:
    if not commits:
        return "No user-facing changes.\n"

    grouped: dict[str, list[str]] = {}
    breaking: list[str] = []
    untyped = 0

    for kind, scope, text, is_breaking in commits:
        if is_breaking:
            breaking.append(f"**{scope}**: {text}" if scope else text)
        if kind in INTERNAL:
            continue
        if kind == "other":
            untyped += 1
        grouped.setdefault(kind, []).append(f"**{scope}**: {text}" if scope else text)

    out: list[str] = []

    if breaking:
        out.append("### Breaking changes\n")
        out += [f"- {b}" for b in breaking]
        out.append("")

    for kind, _label, _ in GROUPS:
        items = grouped.get(kind)
        if not items:
            continue
        # A single item reads better as a sentence than as a one-bullet list.
        if len(items) == 1 and not breaking:
            out.append(items[0] + "\n")
            continue
        out.append(f"### {HEADINGS[kind]}\n")
        out += [f"- {i}" for i in items]
        out.append("")

    leftovers = grouped.get("other") or []
    if leftovers:
        # Its own heading, not a footnote. These are usually the most important
        # lines in the file -- the ones written as a story of what was wrong.
        out.append("### Everything else\n")
        out += [f"- {i}" for i in leftovers]
        out.append("")
        print(
            f"note: {untyped} commit(s) did not use a conventional-commit prefix and "
            f"were listed under 'Everything else'. 'fix: ', 'feat: ' and friends would "
            f"sort them into the right section.",
            file=sys.stderr,
        )

    return "\n".join(out).rstrip() + "\n"


def main() -> int:
    args = [a for a in sys.argv[1:] if not a.startswith("-")]
    since = args[0] if args else None

    commits = collect(since)
    sys.stdout.write(render(commits))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
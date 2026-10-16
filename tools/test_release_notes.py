"""Tests for the changelog generator.

Run:  python tools/test_release_notes.py

Standard library only, like everything else in tools/ that CI runs.

This file exists because the generator shipped a bug that no amount of looking
at its output would have caught: it asked git for `%s%x00`, the NUL separator
never arrived, every commit became one string, and `parse_subject` took the
first line of it. A release covering eight commits listed one -- and listed it
without complaint, because the output of a changelog that has dropped seven of
your changes is indistinguishable from a release that had one change in it.

So the tests are about counting, not about looking right.
"""

import os
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

import release_notes as rn


class Parsing(unittest.TestCase):
    def test_the_conventional_shapes_are_understood(self):
        cases = [
            ("fix: the lightbar only ran steady", ("fix", "", False)),
            ("feat(controller): add the off mode", ("feat", "controller", False)),
            ("fix!: drop the old config key", ("fix", "", True)),
            ("chore: bump a dependency", ("chore", "", False)),
            ("docs: fix a broken link", ("docs", "", False)),
        ]
        for subject, (kind, scope, breaking) in cases:
            with self.subTest(subject=subject):
                parsed = rn.parse_subject(subject)
                self.assertIsNotNone(parsed, subject)
                self.assertEqual(parsed[0], kind)
                self.assertEqual(parsed[1], scope)
                self.assertEqual(parsed[3], breaking)

    def test_a_subject_with_no_type_is_kept_rather_than_dropped(self):
        # The important one. Dropping it produces a confident, complete-looking
        # release note that is missing the change somebody cared about, and
        # nobody notices an absence.
        parsed = rn.parse_subject("Release 1.0.2: a data-loss fix")
        self.assertIsNotNone(parsed)
        self.assertEqual(parsed[0], "other")

    def test_an_unknown_type_is_treated_as_prose_not_as_a_type(self):
        parsed = rn.parse_subject("perfetti: make the lightbar sparkle")
        self.assertIsNotNone(parsed)
        self.assertEqual(parsed[0], "other")

    def test_a_subject_is_becomes_a_sentence(self):
        # Commit subjects are capitalised and unpunctuated. Prose is the
        # opposite, and a changelog full of fragments reads like machine
        # output -- which it is.
        kind, _scope, text, _b = rn.parse_subject("fix: the lightbar did nothing")
        self.assertEqual(text, "The lightbar did nothing.")
        self.assertEqual(kind, "fix")

    def test_an_existing_full_stop_is_not_doubled(self):
        _k, _s, text, _b = rn.parse_subject("fix: it was already punctuated.")
        self.assertEqual(text.count("."), 1)


class Rendering(unittest.TestCase):
    def test_fixes_and_features_land_in_their_own_sections(self):
        out = rn.render(
            [
                ("fix", "", "The lightbar did nothing.", False),
                ("feat", "wheel", "Wheel profiles.", False),
            ]
        )
        self.assertIn("### Fixes", out)
        self.assertIn("### New", out)
        self.assertIn("The lightbar did nothing.", out)

    def test_a_single_change_reads_as_a_sentence_not_a_one_item_list(self):
        out = rn.render([("fix", "", "One thing changed.", False)])
        self.assertNotIn("### Fixes", out)
        self.assertIn("One thing changed.", out)

    def test_two_groups_of_one_still_get_headings(self):
        # Applying the single-item rule per group gives "One." followed by
        # "**wheel**: Wheel profiles." with nothing to tell them apart. The rule
        # is about the whole release, not each section.
        out = rn.render(
            [
                ("fix", "", "One thing changed.", False),
                ("feat", "wheel", "Wheel profiles.", False),
            ]
        )
        self.assertIn("### Fixes", out)
        self.assertIn("### New", out)

    def test_maintenance_commits_are_left_out_but_counted(self):
        # A commit that vanishes between the log and the notes is the failure
        # this script exists to prevent, and "it was only a chore" is not a
        # reason nobody is told.
        out = rn.render(
            [
                ("fix", "", "A real fix.", False),
                ("chore", "", "Bump a dependency.", False),
            ]
        )
        self.assertIn("A real fix.", out)
        self.assertNotIn("Bump a dependency.", out)

    def test_breaking_changes_come_first_and_are_not_hidden(self):
        out = rn.render(
            [
                ("feat", "", "Something new.", False),
                ("fix", "settings", "Old files migrate now.", True),
            ]
        )
        self.assertIn("### Breaking changes", out)
        self.assertLess(out.index("Breaking changes"), out.index("Something new."))

    def test_no_commits_says_so_rather_than_claiming_a_quiet_release(self):
        self.assertIn("No user-facing changes", rn.render([]))

    def test_commits_without_a_type_are_listed_rather_than_dropped(self):
        out = rn.render(
            [
                ("fix", "", "A real fix.", False),
                ("other", "", "A prose commit.", False),
            ]
        )
        self.assertIn("A real fix.", out)
        self.assertIn("A prose commit.", out)
        self.assertIn("Everything else", out)


class Counting(unittest.TestCase):
    """The bug: one commit listed where there were eight."""

    def test_every_commit_in_the_range_appears_in_the_output(self):
        """Build a throwaway repo with N commits and count them out again.

        The point is not that the headings are right. It is that N in means N
        out, which is the property the first version silently did not have.
        """
        original = os.getcwd()
        with tempfile.TemporaryDirectory() as tmp:
            try:
                os.chdir(tmp)
                run = lambda *a: subprocess.run(  # noqa: E731
                    ["git", *a], capture_output=True, check=True
                )
                run("init", "-q")
                run("config", "user.email", "t@example.com")
                run("config", "user.name", "t")

                subjects = [
                    "fix: one",
                    "feat: two",
                    "A prose commit about three",
                    "docs: four",
                    "perf: five",
                    "chore: six",
                    "test: seven",
                    "Release 8: a version bump",
                ]
                for i, s in enumerate(subjects):
                    with open("f", "w", encoding="utf-8") as fh:
                        fh.write(str(i))
                    run("add", "f")
                    run("commit", "-q", "-m", s)

                collected = rn.collect(None)
                out = rn.render(collected)

                self.assertEqual(
                    len(collected),
                    len(subjects),
                    f"collected {len(collected)} of {len(subjects)} commits",
                )
                # Every commit's words must be findable in the output -- except
                # the maintenance ones, which are deliberately left out and
                # counted on stderr instead of vanishing.
                for s in subjects:
                    if rn.parse_subject(s)[0] in rn.INTERNAL:
                        continue
                    body = rn.parse_subject(s)[2]
                    self.assertIn(body, out, f"'{s}' is missing from the notes")

                # And a merge commit must not be counted, or it inflates the
                # count without adding a line.
                run("checkout", "-q", "-b", "side")
                with open("g", "w", encoding="utf-8") as fh:
                    fh.write("g")
                run("add", "g")
                run("commit", "-q", "-m", "feat: a side branch")
                run("checkout", "-q", "-")
                run("merge", "-q", "--no-ff", "-m", "Merge branch 'side'", "side")
                self.assertEqual(
                    len(rn.collect(None)),
                    len(subjects) + 1,
                    "a merge should add exactly its own commit",
                )
            finally:
                os.chdir(original)


if __name__ == "__main__":
    unittest.main(verbosity=2)
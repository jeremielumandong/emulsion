#!/usr/bin/env python3
"""Check retries of abandoned release preparations and duplicate protection."""
import importlib.util
from pathlib import Path
import subprocess
import sys
import unittest
from unittest.mock import patch

sys.dont_write_bytecode = True
spec = importlib.util.spec_from_file_location(
    "preparation", Path(__file__).with_name("release-preparation-branch.py"))
preparation = importlib.util.module_from_spec(spec)
spec.loader.exec_module(preparation)


class PreparationTests(unittest.TestCase):
    def choose(self, refs=(), prs=(), run_id="123", attempt="1"):
        return preparation.select_branch("0.1.0", refs, prs, run_id, attempt)

    def test_first_attempt_uses_standard_branch(self):
        self.assertEqual(self.choose(), "release/v0.1.0")

    def test_closed_or_orphaned_branch_gets_new_attempt(self):
        refs = {"refs/heads/release/v0.1.0"}
        self.assertEqual(self.choose(refs), "release/v0.1.0-prep-123-1")
        refs.add("refs/heads/release/v0.1.0-prep-123-1")
        self.assertEqual(self.choose(refs, attempt="2"), "release/v0.1.0-prep-123-2")

    def test_existing_tag_always_blocks(self):
        with self.assertRaisesRegex(ValueError, "Tag v0.1.0 already exists"):
            self.choose({"refs/tags/v0.1.0"})

    def test_open_pr_blocks_original_and_retry_branches_even_if_branch_deleted(self):
        for branch in ["release/v0.1.0", "release/v0.1.0-prep-123-1"]:
            with self.subTest(branch=branch), self.assertRaisesRegex(ValueError, "pull/20"):
                self.choose(prs=[{"headRefName": branch, "url": "https://example.test/pull/20"}])

    def test_other_versions_do_not_block(self):
        self.assertEqual(self.choose(
            {"refs/tags/v0.0.3", "refs/heads/release/v0.1.1"},
            [{"headRefName": "release/v0.1.1-prep-123-1", "url": "unrelated"}],
        ), "release/v0.1.0")

    def test_never_overwrites_retry_branch(self):
        with self.assertRaisesRegex(ValueError, "already exists"):
            self.choose({"refs/heads/release/v0.1.0", "refs/heads/release/v0.1.0-prep-123-1"})

    def test_retry_requires_valid_run_identifiers(self):
        for run_id, attempt in [("", "1"), ("123", ""), ("bad/ref", "1")]:
            with self.subTest(run_id=run_id, attempt=attempt), self.assertRaises(ValueError):
                self.choose({"refs/heads/release/v0.1.0"}, run_id=run_id, attempt=attempt)

    def test_remote_inspection_is_read_only(self):
        with patch.object(preparation.subprocess, "run", side_effect=[
            subprocess.CompletedProcess([], 0, "abc\trefs/heads/release/v0.1.0\n"),
            subprocess.CompletedProcess([], 0, "[]"),
        ]) as run:
            self.assertEqual(preparation.inspect_branch("0.1.0", "123", "1"),
                             "release/v0.1.0-prep-123-1")
        self.assertEqual(run.call_args_list[0].args[0], ["git", "ls-remote", "--refs", "origin"])
        self.assertEqual(run.call_args_list[1].args[0][:3], ["gh", "pr", "list"])

    def test_lookup_failure_does_not_choose_a_branch(self):
        for results in [
            [subprocess.CalledProcessError(1, "git")],
            [subprocess.CompletedProcess([], 0, ""), subprocess.CalledProcessError(1, "gh")],
        ]:
            with self.subTest(results=results), patch.object(
                preparation.subprocess, "run", side_effect=results
            ), self.assertRaises(subprocess.CalledProcessError):
                preparation.inspect_branch("0.1.0", "123", "1")


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
"""Offline tests for the hash-pinned Codeks release asset downloader."""

from __future__ import annotations

import base64
import hashlib
import io
import json
import os
import sys
import tarfile
import tempfile
import unittest
import urllib.request
from pathlib import Path
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parent))
import codeks_release_asset as assets


class ReleaseAssetTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="codeks-release-assets-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def test_dotslash_matrix_is_hash_pinned_on_all_six_runners(self):
        expected = {
            ("Windows", "X64"), ("Windows", "ARM64"),
            ("Linux", "X64"), ("Linux", "ARM64"),
            ("macOS", "X64"), ("macOS", "ARM64"),
        }
        self.assertEqual(set(assets.DOTSLASH_RELEASE), expected)
        for name, digest in assets.DOTSLASH_RELEASE.values():
            self.assertTrue(name.endswith(".v0.5.9.tar.gz"))
            self.assertRegex(digest, r"^[0-9a-f]{64}$")
        self.assertEqual(assets.DOTSLASH_RELEASE[("macOS", "X64")],
                         assets.DOTSLASH_RELEASE[("macOS", "ARM64")])

    def test_cached_asset_is_verified_without_network(self):
        item = self.root / "cached.tar.gz"
        item.write_bytes(b"checked-payload")
        expected = hashlib.sha256(item.read_bytes()).hexdigest()
        with patch.object(assets, "_retry_open", side_effect=AssertionError("No network expected")):
            self.assertEqual(assets.download_release_asset("example/project", "v1", item.name, expected, item), item)
        with self.assertRaisesRegex(ValueError, "incorrect SHA256"):
            assets.download_release_asset("example/project", "v1", item.name, "0" * 64, item)

    def test_rejects_unsafe_asset_identifiers_and_digest(self):
        item = self.root / "out"
        for repo, tag, name, digest in [
            ("../fake", "v1", "a.tar.gz", "0" * 64),
            ("owner/repo", "v1/else", "a.tar.gz", "0" * 64),
            ("owner/repo", "v1", "../escape", "0" * 64),
            ("owner/repo", "v1", "a.tar.gz", "not-a-digest"),
        ]:
            with self.assertRaises(ValueError):
                assets.download_release_asset(repo, tag, name, digest, item)

    def test_github_token_never_reaches_release_cdn_on_redirect(self):
        original = urllib.request.Request(
            "https://api.github.com/repos/owner/repo/releases/assets/42",
            headers={"Authorization": "Bearer synthetic-token", "Accept": "application/octet-stream"},
        )
        redirected = assets._NoCrossOriginAuthorization().redirect_request(
            original, None, 302, "Found", {},
            "https://release-assets.githubusercontent.com/download/signed",
        )
        self.assertIsNotNone(redirected)
        self.assertFalse(redirected.has_header("Authorization"))
        self.assertTrue(original.has_header("Authorization"))

    def test_bazel_source_must_match_lock_bytes_before_parsing(self):
        digest = hashlib.sha256(b"tar").digest()
        original = json.dumps({
            "url": "https://github.com/bazelbuild/rules_cc/releases/download/0.2.20/rules_cc-0.2.20.tar.gz",
            "integrity": "sha256-" + base64.b64encode(digest).decode("ascii"),
        }).encode()
        checksum = hashlib.sha256(original).hexdigest()
        with patch.object(assets, "_retry_open", return_value=io.BytesIO(original)):
            item = assets.bazel_release_asset_from_locked_source("https://bcr.bazel.build/modules/rules_cc/0.2.20/source.json", checksum)
        self.assertEqual(item, ("bazelbuild/rules_cc", "0.2.20", "rules_cc-0.2.20.tar.gz", digest.hex()))
        with patch.object(assets, "_retry_open", return_value=io.BytesIO(original)):
            with self.assertRaisesRegex(ValueError, "changed since MODULE.bazel.lock"):
                assets.bazel_release_asset_from_locked_source("https://bcr.bazel.build/modules/rules_cc/0.2.20/source.json", "0" * 64)

    def test_dotslash_install_preserves_archive_bytes_and_writes_path(self):
        archive = self.root / "dotslash-windows.v0.5.9.tar.gz"
        content = b"synthetic-executor"
        with tarfile.open(archive, "w:gz") as output:
            info = tarfile.TarInfo("dotslash.exe")
            info.size = len(content)
            output.addfile(info, io.BytesIO(content))
        runner_temp = self.root / "runner"
        github_path = self.root / "github_path.txt"
        with patch.object(assets, "download_release_asset", return_value=archive), \
             patch.object(assets.subprocess, "run") as run:
            output = assets.install_dotslash("Windows", "X64", runner_temp, github_path)
        self.assertEqual(output.read_bytes(), content)
        self.assertEqual(github_path.read_text(encoding="utf-8").strip(), str(output.parent))
        run.assert_called_once()

    def test_bazel_prefetch_rejects_clashing_basename(self):
        (self.root / "MODULE.bazel.lock").write_text(json.dumps({
            "registryFileHashes": {
                "https://bcr.bazel.build/modules/a/1/source.json": "0" * 64,
                "https://bcr.bazel.build/modules/b/1/source.json": "1" * 64,
            },
        }), encoding="utf-8")
        rows = [
            ("owner/a", "v1", "same.tar.gz", "0" * 64),
            ("owner/b", "v1", "same.tar.gz", "1" * 64),
        ]
        with patch.object(assets, "bazel_release_asset_from_locked_source", side_effect=rows):
            with self.assertRaisesRegex(ValueError, "basename collision"):
                assets.prefetch_bazel_release_assets(self.root, self.root / "distdir", dry_run=True)


if __name__ == "__main__":
    unittest.main()

#!/usr/bin/env python3
"""Tests for the standalone Codeks executor archive (codeks 0004)."""

import json
import os
import stat
import sys
import tarfile
import tempfile
import unittest
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import codeks_exec_release as release


VERSION = "0.167.0-codeks.2"


class StandaloneArchiveTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="codeks-exec-release-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def base_package(self, target: str) -> Path:
        base = self.root / "base"
        base.mkdir()
        suffix = ".exe" if "windows" in target else ""
        metadata = {
            "layoutVersion": 1,
            "version": "0.167.0",
            "target": target,
            "variant": "codex",
            "entrypoint": f"bin/codex{suffix}",
            "execServer": f"bin/codex-exec-server{suffix}",
            "resourcesDir": "codex-resources",
            "pathDir": "codex-path",
        }
        (base / "codex-package.json").write_text(json.dumps(metadata), encoding="utf-8")
        files = [f"bin/codex{suffix}", f"bin/codex-code-mode-host{suffix}",
                 f"bin/codex-exec-server{suffix}", f"codex-path/rg{suffix}"]
        if "linux" in target:
            files.append("codex-resources/bwrap")
        if "windows" in target:
            files.extend(["codex-resources/codex-command-runner.exe",
                          "codex-resources/codex-windows-sandbox-setup.exe"])
        (base / "codex-resources").mkdir()
        for relative in files:
            path = base / relative
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(b"synthetic binary for packaging tests")
            if os.name != "nt":
                path.chmod(path.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
        return base

    def test_windows_archive_contains_executor_and_sandbox_but_no_cli(self):
        target = "x86_64-pc-windows-msvc"
        archive = release.package(self.base_package(target), target, VERSION, self.root / "out")
        self.assertEqual(archive.suffix, ".zip")
        self.assertEqual(archive.with_name(archive.name + ".sha256").read_text(),
                         release.checksum_line(archive))
        with zipfile.ZipFile(archive) as source:
            source.extractall(self.root / "unpacked")
        release.verify_layout(self.root / "unpacked", target, VERSION)
        self.assertTrue((self.root / "unpacked" / "codex-resources/codex-command-runner.exe").is_file())
        self.assertFalse((self.root / "unpacked" / "bin/codex.exe").exists())

    @unittest.skipIf(os.name == "nt", "Unix executable permissions are not portable to Windows")
    def test_linux_archive_preserves_bwrap_and_executable_mode(self):
        target = "x86_64-unknown-linux-musl"
        archive = release.package(self.base_package(target), target, VERSION, self.root / "out")
        self.assertEqual(archive.suffixes[-2:], [".tar", ".gz"])
        with tarfile.open(archive) as source:
            source.extractall(self.root / "unpacked", filter="data")
        release.verify_layout(self.root / "unpacked", target, VERSION)
        self.assertTrue(os.access(self.root / "unpacked/codex-resources/bwrap", os.X_OK))
        self.assertFalse((self.root / "unpacked/bin/codex").exists())

    def test_missing_sandbox_helper_is_rejected(self):
        target = "x86_64-pc-windows-msvc"
        base = self.base_package(target)
        (base / "codex-resources/codex-command-runner.exe").unlink()
        with self.assertRaisesRegex(RuntimeError, "Missing package file"):
            release.package(base, target, VERSION, self.root / "out")

    def test_invalid_version_cannot_escape_archive_directory(self):
        with self.assertRaises(ValueError):
            release.archive_name("../../unsafe", "x86_64-pc-windows-msvc")


if __name__ == "__main__":
    unittest.main()

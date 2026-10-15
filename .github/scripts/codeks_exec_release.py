#!/usr/bin/env python3
"""Package and smoke-test standalone Codeks exec-server downloads (codeks 0004).

The native build already produces this binary and its sandbox helpers. Reuse the
verified base package instead of compiling another executable or publishing to npm.
"""

from __future__ import annotations

import argparse
import asyncio
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[2]
os.environ.setdefault("CODEX_REPO_ROOT", str(ROOT))
sys.path.insert(0, str(ROOT / "scripts"))

from codex_package.archive import write_archive
from codex_package.layout import validate_package_dir
from codex_package.targets import PACKAGE_VARIANTS, TARGET_SPECS


def archive_name(release_version: str, target: str) -> str:
    if not re.fullmatch(r"[A-Za-z0-9._-]+", release_version):
        raise ValueError(f"Invalid release version: {release_version!r}")
    if target not in TARGET_SPECS:
        raise ValueError(f"Unknown target: {target}")
    extension = ".zip" if "windows" in target else ".tar.gz"
    return f"codeks-exec-server-{release_version}-{target}{extension}"


def executor_path(target: str) -> Path:
    return Path("bin") / ("codex-exec-server.exe" if "windows" in target else "codex-exec-server")


def checksum_line(archive: Path) -> str:
    with archive.open("rb") as source:
        digest = hashlib.file_digest(source, "sha256").hexdigest()
    return f"{digest}  {archive.name}\n"


def verify_layout(package: Path, target: str, release_version: str) -> dict:
    metadata = json.loads((package / "codex-package.json").read_text(encoding="utf-8"))
    exe = executor_path(target)
    expected = {
        "layoutVersion": 1,
        "version": release_version.split("-codeks.", 1)[0],
        "target": target,
        "variant": "exec-server",
        "entrypoint": exe.as_posix(),
        "execServer": exe.as_posix(),
        "resourcesDir": "codex-resources",
        "pathDir": "codex-path",
    }
    for key, value in expected.items():
        if metadata.get(key) != value:
            raise ValueError(f"Standalone metadata {key}: expected {value!r}, got {metadata.get(key)!r}")
    required = [exe, Path("codex-package.json"), Path("LICENSE"), Path("codex-path") / ("rg.exe" if "windows" in target else "rg")]
    if "linux" in target:
        required.append(Path("codex-resources/bwrap"))
    if "windows" in target:
        required.extend([Path("codex-resources/codex-command-runner.exe"), Path("codex-resources/codex-windows-sandbox-setup.exe")])
    for relative in required:
        path = package / relative
        if not path.is_file() or path.is_symlink():
            raise ValueError(f"Missing or invalid standalone resource: {relative}")
        if "windows" not in target and relative in (exe, Path("codex-path/rg"), Path("codex-resources/bwrap")) and not os.access(path, os.X_OK):
            raise ValueError(f"Standalone resource is not executable: {relative}")
    if any((package / extra).exists() for extra in ("bin/codex", "bin/codex.exe", "bin/codex-code-mode-host", "bin/codex-code-mode-host.exe", "codex-resources/voice")):
        raise ValueError("Standalone package unexpectedly contains the CLI, code-mode host, or voice runtime")
    return metadata


def package(base: Path, target: str, release_version: str, output_dir: Path) -> Path:
    archive_name(release_version, target)
    base = base.resolve(strict=True)
    validate_package_dir(base, PACKAGE_VARIANTS["codex"], TARGET_SPECS[target])
    original = json.loads((base / "codex-package.json").read_text(encoding="utf-8"))
    if original["version"] != release_version.split("-codeks.", 1)[0]:
        raise ValueError("Base package version does not match the release version")
    with tempfile.TemporaryDirectory(prefix="codeks-exec-package-") as directory:
        staged = Path(directory) / "standalone"
        (staged / "bin").mkdir(parents=True)
        exe = executor_path(target)
        shutil.copy2(base / exe, staged / exe)
        # Keep the exact sandbox helper bytes and rg from the same native build.
        shutil.copytree(base / "codex-resources", staged / "codex-resources")
        shutil.copytree(base / "codex-path", staged / "codex-path")
        shutil.copy2(ROOT / "LICENSE", staged / "LICENSE")
        original.update(variant="exec-server", entrypoint=exe.as_posix())
        (staged / "codex-package.json").write_text(json.dumps(original, indent=2) + "\n", encoding="utf-8")
        verify_layout(staged, target, release_version)
        output_dir.mkdir(parents=True, exist_ok=True)
        result = output_dir / archive_name(release_version, target)
        write_archive(staged, result, force=False)
    (output_dir / (result.name + ".sha256")).write_text(checksum_line(result), encoding="utf-8")
    print(f"Standalone exec-server: {result}")
    return result


async def smoke_stdio(executable: Path, cwd: Path, version: str) -> None:
    home = cwd / "codeks-exec-smoke-home"
    home.mkdir()
    env = dict(os.environ, CODEX_HOME=str(home))
    process = await asyncio.create_subprocess_exec(
        str(executable), "--listen", "stdio", cwd=str(cwd), env=env,
        stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
        stderr=asyncio.subprocess.PIPE,
    )
    try:
        assert process.stdin is not None and process.stdout is not None
        request = {"id": 1, "method": "initialize", "params": {"clientName": "codeks-release-smoke"}}
        process.stdin.write((json.dumps(request) + "\n").encode())
        await process.stdin.drain()
        response_line = await asyncio.wait_for(process.stdout.readline(), timeout=30)
        response = json.loads(response_line)
        if response.get("id") != 1 or not isinstance(response.get("result", {}).get("sessionId"), str):
            raise ValueError(f"Standalone initialize failed: {response}")
        if response["result"]["environmentInfo"]["executorVersion"] != version:
            raise ValueError("Standalone executor version differs from its archive metadata")
        process.stdin.close()
        await asyncio.wait_for(process.wait(), timeout=20)
        if process.returncode != 0:
            raise RuntimeError(f"Standalone exec-server exited with {process.returncode}")
    finally:
        if process.returncode is None:
            process.kill()
            await process.wait()


def smoke(archives_dir: Path, target: str, release_version: str) -> None:
    archive = archives_dir / archive_name(release_version, target)
    expected_checksum = checksum_line(archive)
    if (archives_dir / (archive.name + ".sha256")).read_text(encoding="utf-8") != expected_checksum:
        raise ValueError(f"Checksum mismatch for {archive}")
    with tempfile.TemporaryDirectory(prefix="codeks-exec-smoke-") as directory:
        unpacked = Path(directory)
        if archive.suffix == ".zip":
            with zipfile.ZipFile(archive) as source:
                for member in source.namelist():
                    if PurePosixPath(member).is_absolute() or ".." in PurePosixPath(member).parts:
                        raise ValueError("Unsafe path in standalone ZIP")
                source.extractall(unpacked)
        else:
            with tarfile.open(archive) as source:
                source.extractall(unpacked, filter="data")
        metadata = verify_layout(unpacked, target, release_version)
        executable = unpacked / executor_path(target)
        if sys.platform == "darwin":
            subprocess.run(["codesign", "--verify", "--strict", str(executable)], check=True)
        env = dict(os.environ, CODEX_HOME=str(unpacked))
        result = subprocess.run([str(executable), "--version"], cwd=unpacked, env=env,
                                capture_output=True, text=True, timeout=30, check=True)
        expected_version = f"codex-exec-server {metadata['version']}"
        if result.stdout.strip() != expected_version:
            raise ValueError(f"Expected {expected_version!r}, got {result.stdout!r}")
        asyncio.run(smoke_stdio(executable, unpacked, metadata["version"]))
        print(f"Verified native standalone exec-server: {target} / {release_version}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    for command in ("package", "smoke"):
        p = sub.add_parser(command)
        p.add_argument("--target", required=True, choices=sorted(TARGET_SPECS))
        p.add_argument("--release-version", required=True)
        if command == "package":
            p.add_argument("--base-dir", type=Path, required=True)
            p.add_argument("--output-dir", type=Path, required=True)
        else:
            p.add_argument("--archives-dir", type=Path, required=True)
    args = parser.parse_args()
    if args.command == "package":
        package(args.base_dir, args.target, args.release_version, args.output_dir)
    else:
        smoke(args.archives_dir, args.target, args.release_version)


if __name__ == "__main__":
    main()

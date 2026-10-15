"""Verify complete release packages and initialize their native voice runtime."""

from __future__ import annotations

import asyncio
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]


def verify_package(package: Path, target: str, version: str, commit: str) -> None:
    """Reuse upstream layout and runtime checks, then verify the assembled receipt."""
    package = package.resolve(strict=True)
    os.environ.setdefault("CODEX_REPO_ROOT", str(ROOT))
    sys.path.insert(0, str(ROOT / "scripts"))
    sys.path.insert(0, str(ROOT / "third_party" / "voice"))
    from codex_package.layout import validate_package_dir
    from codex_package.targets import PACKAGE_VARIANTS, TARGET_SPECS
    from package_runtime import runtime_files

    validate_package_dir(package, PACKAGE_VARIANTS["codex"], TARGET_SPECS[target])
    metadata = json.loads((package / "codex-package.json").read_text(encoding="utf-8"))
    if metadata["version"] != version:
        raise ValueError(f"Package version {metadata['version']!r} does not match {version!r}")
    voice = package / "codex-resources" / "voice"
    manifest = json.loads((voice / "manifest.json").read_text(encoding="utf-8"))
    voice_target = target.replace("-musl", "-gnu")
    expected = {
        "schemaVersion": 1,
        "buildCommit": commit,
        "appVersion": version,
        "appTarget": target,
        "voiceTarget": voice_target,
    }
    for key, value in expected.items():
        if manifest.get(key) != value:
            raise ValueError(f"Voice manifest {key} does not match {value!r}")
    runtime = json.loads((voice / "runtime.json").read_text(encoding="utf-8"))
    if runtime["sourceCommit"] != commit:
        raise ValueError("Voice runtime does not match the package build commit")
    files = runtime_files(voice, voice_target, public_release=True)
    suffix = ".exe" if "windows" in target else ""
    required = {
        metadata["entrypoint"],
        f"codex-resources/voice/bin/codex-voice-host{suffix}",
        *(f"codex-resources/voice/{name}" for name in files),
    }
    if not required.issubset(manifest["sha256"]):
        raise ValueError("Voice manifest does not cover the app, helper, and runtime")
    for relative, expected_digest in manifest["sha256"].items():
        path = package / relative
        if not path.resolve().is_relative_to(package.resolve()):
            raise ValueError(f"Manifest path escapes the package: {relative}")
        with path.open("rb") as source:
            actual = hashlib.file_digest(source, "sha256").hexdigest()
        if actual != expected_digest:
            raise ValueError(f"Package digest mismatch: {relative}")


def smoke_voice(package: Path, target: str, commit: str) -> None:
    """Exercise the same-build handshake and native initialization without devices."""
    suffix = ".exe" if "windows" in target else ""
    helper = package / "codex-resources" / "voice" / "bin" / f"codex-voice-host{suffix}"
    if target.endswith("apple-darwin"):
        binaries = [
            package / "bin" / "codex",
            package / "bin" / "codex-code-mode-host",
            package / "codex-path" / "rg",
            helper,
            *(package / "codex-resources" / "voice").rglob("*.dylib"),
        ]
        for binary in binaries:
            subprocess.run(["codesign", "--verify", "--strict", str(binary)], check=True)
    identity = subprocess.check_output([str(helper), "--build-commit"], text=True, timeout=30).strip()
    if identity != commit:
        raise ValueError(f"Voice helper build {identity!r} does not match {commit!r}")
    # Match the native loader's fixed environment; no device or network setup is needed.
    env = os.environ | {
        "GST_PLUGIN_PATH": "",
        "GST_PLUGIN_PATH_1_0": "",
        "GST_PLUGIN_SYSTEM_PATH": "",
        "GST_PLUGIN_SYSTEM_PATH_1_0": "",
        "GST_REGISTRY": "NUL" if "windows" in target else "/dev/null",
        "GST_REGISTRY_UPDATE": "no",
        "GST_REGISTRY_FORK": "no",
    }

    async def exchange() -> None:
        process = await asyncio.create_subprocess_exec(
            str(helper), cwd=package, env=env,
            stdin=asyncio.subprocess.PIPE, stdout=asyncio.subprocess.PIPE,
        )
        try:
            assert process.stdin is not None and process.stdout is not None
            requests = (
                ({"type": "hello", "protocol": 1, "buildCommit": commit}, "ready"),
                ({"type": "initializeRuntime"}, "runtimeReady"),
                ({"type": "close"}, "closed"),
            )
            for request, response in requests:
                payload = json.dumps(request).encode()
                process.stdin.write(len(payload).to_bytes(4, "big") + payload)
                await process.stdin.drain()
                size = int.from_bytes(await process.stdout.readexactly(4), "big")
                if not 0 < size <= 128 * 1024:
                    raise ValueError(f"Invalid voice response frame length: {size}")
                message = json.loads(await process.stdout.readexactly(size))
                if message != {"type": response}:
                    raise ValueError(f"Expected voice {response}, received {message!r}")
            if await process.wait() != 0:
                raise ValueError(f"Voice helper exited with status {process.returncode}")
        finally:
            if process.returncode is None:
                process.kill()
                await process.wait()

    async def bounded_exchange() -> None:
        await asyncio.wait_for(exchange(), timeout=30)

    asyncio.run(bounded_exchange())

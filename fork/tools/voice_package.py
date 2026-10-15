#!/usr/bin/env python3
"""Stage and assemble voice resources with the upstream release scripts."""

import argparse
from pathlib import Path
import shutil
import stat
import subprocess
import sys
import tomllib


ROOT = Path(__file__).resolve().parents[2]
VOICE = ROOT / "third_party" / "voice"
TARGETS = {
    f"{arch}-{suffix}": f"{arch}-{suffix.replace('musl', 'gnu')}"
    for arch in ("x86_64", "aarch64")
    for suffix in ("unknown-linux-musl", "pc-windows-msvc")
}


def upstream(script: str, *args: str | Path) -> None:
    subprocess.run(
        [sys.executable, str(VOICE / script), *(str(arg) for arg in args)],
        cwd=ROOT,
        check=True,
    )


def helper_name(target: str) -> str:
    return "codex-voice-host" + (".exe" if target.endswith("windows-msvc") else "")


def stage(args: argparse.Namespace) -> None:
    target = TARGETS[args.target]
    output = args.output.resolve()
    output.mkdir(parents=True)
    runtime = output / "runtime"
    upstream(
        "release_runtime.py", "stage",
        "--target", target, "--source", args.source.resolve(), "--output", runtime,
    )
    helper = output / helper_name(args.target)
    shutil.copy2(args.helper, helper)
    if target.endswith("windows-msvc"):
        # The upstream script downloads and verifies the pinned retail CRT.
        # dumpbin must be available in the runner's MSVC environment.
        receipt = runtime / "runtime.json"
        receipt.chmod(receipt.stat().st_mode | stat.S_IWUSR)
        upstream("windows_crt.py", "--root", runtime, "--target", target, "--helper", helper)
    else:
        helper.chmod(0o755)
    upstream("release_runtime.py", "seal", "--target", target, "--output", runtime)


def assemble(args: argparse.Namespace) -> None:
    with (ROOT / "codex-rs" / "Cargo.toml").open("rb") as source:
        version = tomllib.load(source)["workspace"]["package"]["version"]
    commit = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True,
    ).strip()
    voice_dir = args.voice_dir.resolve()
    output = args.output_dir.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    upstream(
        "assemble_package.py",
        "--package", args.package_dir.resolve(),
        "--helper", voice_dir / helper_name(args.target),
        "--runtime", voice_dir / "runtime",
        "--voice-target", TARGETS[args.target],
        "--build-commit", commit,
        "--release-version", version,
        "--output", output,
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    staging = commands.add_parser("stage", help="Prepare a sealed public-release voice runtime")
    staging.add_argument("--target", choices=TARGETS, required=True)
    staging.add_argument("--source", type=Path, required=True)
    staging.add_argument("--helper", type=Path, required=True)
    staging.add_argument("--output", type=Path, required=True)
    staging.set_defaults(run=stage)
    assembly = commands.add_parser("assemble", help="Add voice to a canonical Codex package")
    assembly.add_argument("--target", choices=TARGETS, required=True)
    assembly.add_argument("--package-dir", type=Path, required=True)
    assembly.add_argument("--voice-dir", type=Path, required=True)
    assembly.add_argument("--output-dir", type=Path, required=True)
    assembly.set_defaults(run=assemble)
    args = parser.parse_args()
    args.run(args)


if __name__ == "__main__":
    main()

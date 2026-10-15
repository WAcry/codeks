#!/usr/bin/env python3
"""Build, check, and publish the codeks npm packages.

The upstream script codex-cli/scripts/build_npm_package.py stages the
packages. This wrapper gives them the fork name and version, keeps only the
platforms that were built, and runs npm pack. The workflow
.github/workflows/codeks-npm.yml (patch 0004) calls it. The version scheme
and the consumer setup are described in fork/README.md (Chinese).

Commands:
  version  print version=... and publishable=... lines for GITHUB_OUTPUT
  pack     stage and pack the packages from a vendor directory
  smoke    unpack the tarballs into a temporary node_modules and run codex --version
  publish  publish the tarballs, platform packages first
"""

from __future__ import annotations

import argparse
import importlib.util
import json
import os
import platform
import re
import subprocess
import sys
import tarfile
import tempfile
from pathlib import Path

try:
    import tomllib
except ModuleNotFoundError:
    sys.exit("npm_package.py needs Python 3.11 or newer")

NPM_NAME = "@wacry/codeks"
UPSTREAM_NPM_NAME = "@openai/codex"
DESCRIPTION = "Personal build of the Codex CLI from the codeks fork (WAcry/codeks). Not an official OpenAI package."
REPOSITORY = {"type": "git", "url": "git+https://github.com/WAcry/codeks.git", "directory": "codex-cli"}
MAIN_DIST_TAG = "latest"
MANIFEST = "codeks-npm.json"
FORK_TAG = re.compile(r"codeks/rust-v(?P<version>[^/]+)/(?P<n>[1-9][0-9]*)")
ROOT = Path(__file__).resolve().parents[2]
BUILD_SCRIPT = ROOT / "codex-cli" / "scripts" / "build_npm_package.py"
HOST_TARGETS = {
    ("Linux", "x86_64"): "x86_64-unknown-linux-musl",
    ("Linux", "aarch64"): "aarch64-unknown-linux-musl",
    ("Darwin", "x86_64"): "x86_64-apple-darwin",
    ("Darwin", "arm64"): "aarch64-apple-darwin",
}


class Fail(Exception):
    """A user-facing error; main() prints it without a traceback."""


def run(cmd: list[str], *, cwd: Path = ROOT) -> str:
    proc = subprocess.run(cmd, cwd=cwd, text=True, capture_output=True)
    if proc.returncode != 0:
        detail = (proc.stderr or proc.stdout).strip()
        raise Fail(f"{' '.join(cmd)} 失败：\n{detail}")
    return proc.stdout.strip()


def git(*args: str) -> str:
    return run(["git", *args])


# --- version -----------------------------------------------------------------


def workspace_version() -> str:
    data = tomllib.loads((ROOT / "codex-rs" / "Cargo.toml").read_text(encoding="utf-8"))
    return data["workspace"]["package"]["version"]


def default_tag() -> str | None:
    """The codeks tag being built: the GitHub tag ref in CI, else a tag at HEAD."""
    if os.environ.get("GITHUB_ACTIONS") == "true":
        if os.environ.get("GITHUB_REF_TYPE") == "tag":
            return os.environ.get("GITHUB_REF_NAME")
        return None
    tags = git("tag", "--points-at", "HEAD", "--list", "codeks/*").split()
    return tags[0] if tags else None


def resolve_version(tag: str | None) -> tuple[str, str | None]:
    """Return the npm version and the fork tag (None for a development build).

    codeks/rust-v0.162.1/3 -> 0.162.1-codeks.3; other refs -> 0.162.1-codeks.dev.g<sha>.
    """
    upstream = workspace_version()
    joiner = "." if "-" in upstream else "-"
    if tag is None:
        return f"{upstream}{joiner}codeks.dev.g{git('rev-parse', 'HEAD')[:7]}", None
    match = FORK_TAG.fullmatch(tag)
    if match is None:
        raise Fail(f"{tag} 不是 codeks/rust-v<版本>/<n> 形式的 tag；npm 包只从 rust-v* 基线发布")
    if match["version"] != upstream:
        raise Fail(f"{tag} 的基线版本 {match['version']} 和 codex-rs/Cargo.toml 的 {upstream} 不一致")
    tagged = git("rev-parse", "--verify", "--quiet", f"{tag}^{{commit}}")
    if tagged and tagged != git("rev-parse", "HEAD"):
        raise Fail(f"HEAD 不是 {tag} 指向的提交")
    return f"{upstream}{joiner}codeks.{match['n']}", tag


def provenance(tag: str | None) -> dict:
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import codeks

    os.chdir(ROOT)
    base = codeks.read_stack().base
    upstream = workspace_version()
    return {
        "tag": tag,
        "commit": git("rev-parse", "HEAD"),
        "upstreamVersion": upstream,
        "upstreamTag": f"rust-v{upstream}" if tag else codeks.base_name(base),
        "upstreamCommit": base,
    }


# --- pack --------------------------------------------------------------------


def upstream_platforms() -> dict[str, dict[str, str]]:
    """CODEX_PLATFORM_PACKAGES from the upstream build script, keyed by Rust target."""
    spec = importlib.util.spec_from_file_location("codex_build_npm_package", BUILD_SCRIPT)
    if spec is None or spec.loader is None:
        raise Fail(f"无法加载 {BUILD_SCRIPT}")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    packages = getattr(module, "CODEX_PLATFORM_PACKAGES", None)
    if not isinstance(packages, dict):
        raise Fail("上游 build_npm_package.py 里没有 CODEX_PLATFORM_PACKAGES 了，按补丁 0004 文档「何时扩展」调整")
    return {config["target_triple"]: {"package": name, **config} for name, config in packages.items()}


def stage(package: str, version: str, staging: Path, vendor: Path | None = None) -> dict:
    cmd = [sys.executable, str(BUILD_SCRIPT), "--package", package, "--version", version,
           "--staging-dir", str(staging)]
    if vendor is not None:
        cmd += ["--vendor-src", str(vendor)]
    run(cmd)
    data = json.loads((staging / "package.json").read_text(encoding="utf-8"))
    if data.get("name") != UPSTREAM_NPM_NAME:
        raise Fail(f"上游 {package} 包名变成了 {data.get('name')!r}，按补丁 0004 文档「何时扩展」调整")
    data["name"] = NPM_NAME
    data["repository"] = REPOSITORY
    return data


def write_and_pack(staging: Path, data: dict, output: Path) -> str:
    (staging / "package.json").write_text(json.dumps(data, indent=2) + "\n", encoding="utf-8")
    packed = json.loads(run(["npm", "pack", "--json", "--pack-destination", str(output)], cwd=staging))
    return packed[0]["filename"]


def cmd_pack(args: argparse.Namespace) -> int:
    version, tag = resolve_version(args.tag or default_tag())
    vendor = args.vendor_src.resolve()
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    platforms = upstream_platforms()
    targets = sorted(path.name for path in vendor.iterdir() if path.is_dir())
    unknown = [target for target in targets if target not in platforms]
    if not targets or unknown:
        raise Fail(f"{vendor} 里应该只有这些 target 目录：{', '.join(sorted(platforms))}；实际是 {targets}")

    entries: list[dict] = []
    with tempfile.TemporaryDirectory(prefix="codeks-npm-") as temp:
        for target in targets:
            info = platforms[target]
            staging = Path(temp) / info["package"]
            data = stage(info["package"], version, staging, vendor)
            entries.append({"file": write_and_pack(staging, data, output), "name": NPM_NAME,
                            "version": data["version"], "tag": info["npm_tag"],
                            "target": target, "alias": info["npm_name"]})

        staging = Path(temp) / "codex"
        data = stage("codex", version, staging)
        upstream_deps = data.get("optionalDependencies", {})
        deps = {}
        for entry in entries:
            expected = f"npm:{UPSTREAM_NPM_NAME}@{entry['version']}"
            if upstream_deps.get(entry["alias"]) != expected:
                raise Fail(f"上游 optionalDependencies 的格式变了：{entry['alias']} = "
                           f"{upstream_deps.get(entry['alias'])!r}，预期 {expected!r}")
            deps[entry["alias"]] = f"npm:{NPM_NAME}@{entry['version']}"
        data["optionalDependencies"] = deps
        data["description"] = DESCRIPTION
        data["codeks"] = provenance(tag)
        entries.append({"file": write_and_pack(staging, data, output), "name": NPM_NAME,
                        "version": version, "tag": MAIN_DIST_TAG})

    manifest = {"version": version, "tag": tag, "packages": entries}
    (output / MANIFEST).write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    for entry in entries:
        print(f"{entry['file']}  ->  {entry['name']}@{entry['version']}（dist-tag {entry['tag']}）")
    return 0


# --- smoke test --------------------------------------------------------------


def extract(tarball: Path, dest: Path) -> None:
    """Unpack an npm tarball (files under package/) into dest."""
    with tarfile.open(tarball) as archive:
        members = []
        for member in archive.getmembers():
            if not member.name.startswith("package/"):
                raise Fail(f"{tarball.name} 里有意外的路径：{member.name}")
            member.name = member.name[len("package/"):]
            members.append(member)
        archive.extractall(dest, members=members, filter="data")


def cmd_smoke(args: argparse.Namespace) -> int:
    folder = args.dir.resolve()
    manifest = json.loads((folder / MANIFEST).read_text(encoding="utf-8"))
    target = HOST_TARGETS.get((platform.system(), platform.machine()))
    main_entry = next(entry for entry in manifest["packages"] if "target" not in entry)
    native = [entry for entry in manifest["packages"] if entry.get("target") == target]
    if not native:
        raise Fail(f"没有当前平台（{target}）的包")
    with tempfile.TemporaryDirectory(prefix="codeks-npm-smoke-") as temp:
        modules = Path(temp) / "node_modules"
        extract(folder / main_entry["file"], modules / NPM_NAME)
        # npm installs each optional dependency under its alias; bin/codex.js resolves it by that name.
        extract(folder / native[0]["file"], modules / native[0]["alias"])
        data = json.loads((modules / NPM_NAME / "package.json").read_text(encoding="utf-8"))
        expected = f"codex-cli {data['codeks']['upstreamVersion']}"
        actual = run(["node", str(modules / NPM_NAME / "bin" / "codex.js"), "--version"], cwd=Path(temp))
    if actual != expected:
        raise Fail(f"codex --version 输出 {actual!r}，预期 {expected!r}")
    print(f"smoke 通过：{NPM_NAME}@{manifest['version']} 运行 {actual}")
    return 0


# --- publish -----------------------------------------------------------------


def cmd_publish(args: argparse.Namespace) -> int:
    folder = args.dir.resolve()
    manifest = json.loads((folder / MANIFEST).read_text(encoding="utf-8"))
    if not manifest["tag"]:
        raise Fail("这是开发构建（不是从 codeks/* tag 构建的），不能发布")
    # The main package goes last, so that its optional dependencies already exist when it becomes latest.
    for entry in manifest["packages"]:
        spec = f"{entry['name']}@{entry['version']}"
        found = subprocess.run(["npm", "view", spec, "version"], text=True, capture_output=True)
        if found.returncode == 0 and found.stdout.strip() == entry["version"]:
            print(f"已经发布过，跳过：{spec}")
            continue
        cmd = ["npm", "publish", str(folder / entry["file"]), "--tag", entry["tag"], "--access", "public"]
        print("+ " + " ".join(cmd), flush=True)
        if subprocess.run(cmd).returncode != 0:
            raise Fail(f"发布 {spec} 失败")
    print(f"已发布 {NPM_NAME}@{manifest['version']}")
    return 0


def cmd_version(args: argparse.Namespace) -> int:
    version, tag = resolve_version(args.tag or default_tag())
    if args.require_tag and tag is None:
        raise Fail("只有 codeks/* tag 可以发布到 npm：用 codeks.py npm <tag> --publish")
    print(f"version={version}")
    print(f"publishable={'true' if tag else 'false'}")
    return 0


def main(argv: list[str] | None = None) -> int:
    sys.stdout.reconfigure(line_buffering=True)
    parser = argparse.ArgumentParser(prog="npm_package.py", description="codeks npm 包，说明见 fork/README.md")
    sub = parser.add_subparsers(dest="command", required=True)

    p = sub.add_parser("version", help="输出 npm 版本号")
    p.add_argument("--tag", help="codeks/* tag，默认取 CI 的 tag ref 或 HEAD 上的 tag")
    p.add_argument("--require-tag", action="store_true", help="不是 tag 构建时报错")
    p.set_defaults(func=cmd_version)

    p = sub.add_parser("pack", help="生成 npm tarball")
    p.add_argument("--vendor-src", type=Path, required=True, help="每个 target 一个 codex package 目录")
    p.add_argument("--output-dir", type=Path, required=True)
    p.add_argument("--tag", help="codeks/* tag，默认取 CI 的 tag ref 或 HEAD 上的 tag")
    p.set_defaults(func=cmd_pack)

    p = sub.add_parser("smoke", help="解包到临时 node_modules 并运行 codex --version")
    p.add_argument("--dir", type=Path, required=True, help="pack 的输出目录")
    p.set_defaults(func=cmd_smoke)

    p = sub.add_parser("publish", help="发布 pack 的输出（需要 npm 凭据）")
    p.add_argument("--dir", type=Path, required=True, help="pack 的输出目录")
    p.set_defaults(func=cmd_publish)

    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except Fail as err:
        print(f"错误：{err}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

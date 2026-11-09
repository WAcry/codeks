#!/usr/bin/env python3
"""Fetch hash-pinned public GitHub release assets via the REST asset endpoint.

Codeks uses this for upstream build tools whose github.com/releases/download
URLs occasionally return HTTP 504. No third-party mirror or mutable fallback.
"""

from __future__ import annotations

import argparse
import base64
import concurrent.futures
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


DOTSLASH_VERSION = "v0.5.9"
DOTSLASH_RELEASE = {
    ("Linux", "X64"): ("dotslash-linux-musl.x86_64.v0.5.9.tar.gz", "4c75c6eb7890ae35993b962073f6d9bbe78b42b81a5691303ad70f63bfbf7196"),
    ("Linux", "ARM64"): ("dotslash-linux-musl.arm64.v0.5.9.tar.gz", "e58374446dacd4a228388cd275d974bbd8f946916b513cf0eb86bdff5d9d675e"),
    ("macOS", "X64"): ("dotslash-macos.v0.5.9.tar.gz", "67963df97f5ce512f900a85950add6e883722c3460ee5d752a6e62665fd2f986"),
    ("macOS", "ARM64"): ("dotslash-macos.v0.5.9.tar.gz", "67963df97f5ce512f900a85950add6e883722c3460ee5d752a6e62665fd2f986"),
    ("Windows", "X64"): ("dotslash-windows.v0.5.9.tar.gz", "c925840a9bf9105a187cd120278cb023c51dac0cecb3234855152422751b7b41"),
    ("Windows", "ARM64"): ("dotslash-windows-arm64.v0.5.9.tar.gz", "43d49d3472808738993ccf95a36dda8cc2869996d9a397bb02bde1e39b16146b"),
}

GITHUB_RELEASE_SOURCE = re.compile(
    r"^https://github\.com/([^/]+/[^/]+)/releases/download/([^/]+)/([^/?#]+)$"
)


def _github_request(url: str, *, accept: str, token: str | None = None):
    headers = {"Accept": accept, "User-Agent": "Codeks-public-release-build"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    return urllib.request.Request(url, headers=headers)


def _retry_open(request: urllib.request.Request):
    for attempt in range(4):
        try:
            return urllib.request.urlopen(request, timeout=120)
        except urllib.error.HTTPError as exc:
            if exc.code not in (408, 429, 500, 502, 503, 504) or attempt == 3:
                raise
        except (urllib.error.URLError, TimeoutError):
            if attempt == 3:
                raise
        time.sleep(2 ** attempt)
    raise AssertionError("unreachable")


def sha256(path: Path) -> str:
    with path.open("rb") as file:
        return hashlib.file_digest(file, "sha256").hexdigest()


def download_release_asset(repo: str, tag: str, asset_name: str, expected_sha256: str, output: Path) -> Path:
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repo):
        raise ValueError("Invalid GitHub repository")
    if any(part in (".", "..") for part in repo.split("/")):
        raise ValueError("Invalid GitHub repository path segment")
    if not tag or "/" in tag or not asset_name or "/" in asset_name or "\\" in asset_name:
        raise ValueError("Invalid release tag or asset name")
    if tag in (".", "..") or asset_name in (".", ".."):
        raise ValueError("Invalid release tag or asset name")
    if not re.fullmatch(r"[0-9a-fA-F]{64}", expected_sha256):
        raise ValueError("Expected pinned 64-character SHA256")
    expected_sha256 = expected_sha256.lower()
    output = output.resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists():
        if sha256(output) != expected_sha256:
            raise ValueError(f"Existing release file has incorrect SHA256: {output}")
        print(f"Verified cached {repo}@{tag} / {asset_name}")
        return output

    metadata_url = f"https://api.github.com/repos/{repo}/releases/tags/{urllib.parse.quote(tag)}"
    metadata_token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    with _retry_open(_github_request(metadata_url, accept="application/vnd.github+json", token=metadata_token)) as response:
        release = json.load(response)
    if release.get("tag_name") != tag:
        raise ValueError(f"Unexpected release tag for {repo}: {release.get('tag_name')}")
    matches = [asset for asset in release["assets"] if asset["name"] == asset_name]
    if len(matches) != 1:
        raise ValueError(f"Expected exactly one {asset_name!r} asset in {repo}@{tag}, got {len(matches)}")
    asset = matches[0]
    declared_digest = asset.get("digest")
    if declared_digest and declared_digest.lower() != f"sha256:{expected_sha256}":
        raise ValueError(f"Release asset metadata SHA256 differs from the pinned digest: {asset_name}")

    # The binary is public. Do not forward a GitHub token to a redirect host.
    asset_url = f"https://api.github.com/repos/{repo}/releases/assets/{asset['id']}"
    fd, temporary = tempfile.mkstemp(prefix=f".{asset_name}.", suffix=".partial", dir=output.parent)
    try:
        with os.fdopen(fd, "wb") as file, _retry_open(_github_request(asset_url, accept="application/octet-stream")) as response:
            shutil.copyfileobj(response, file)
        candidate = Path(temporary)
        if candidate.stat().st_size != asset["size"] or sha256(candidate) != expected_sha256:
            raise ValueError(f"Downloaded asset failed size or SHA256 verification: {repo}@{tag}/{asset_name}")
        os.replace(candidate, output)
    finally:
        Path(temporary).unlink(missing_ok=True)
    print(f"Verified official asset: {repo}@{tag}/{asset_name} ({asset['size']} bytes)")
    return output


def install_dotslash(runner_os: str, runner_arch: str, runner_temp: Path, github_path: Path) -> Path:
    try:
        asset_name, digest = DOTSLASH_RELEASE[(runner_os, runner_arch)]
    except KeyError as exc:
        raise ValueError(f"Unsupported runner OS/architecture: {runner_os}/{runner_arch}") from exc
    binary_name = "dotslash.exe" if runner_os == "Windows" else "dotslash"
    bin_dir = (runner_temp / "codeks-dotslash" / "bin").resolve()
    archive = download_release_asset("facebook/dotslash", DOTSLASH_VERSION, asset_name, digest, bin_dir.parent / asset_name)
    binary_path = bin_dir / binary_name
    bin_dir.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive, "r:gz") as source:
        files = [member for member in source.getmembers() if member.isfile()]
        if len(files) != 1 or files[0].name != binary_name:
            raise ValueError(f"Unexpected DotSlash archive layout: {asset_name}")
        with source.extractfile(files[0]) as stream, binary_path.open("wb") as output:
            shutil.copyfileobj(stream, output)
    if runner_os != "Windows":
        binary_path.chmod(0o755)
    subprocess.run([str(binary_path), "--version"], check=True, timeout=20)
    github_path.parent.mkdir(parents=True, exist_ok=True)
    with github_path.open("a", encoding="utf-8") as stream:
        stream.write(str(bin_dir) + "\n")
    print(f"Added verified DotSlash to GITHUB_PATH: {bin_dir}")
    return binary_path


def bazel_release_asset_from_locked_source(url: str, locked_sha256: str):
    """Only trust registry metadata whose exact bytes match MODULE.bazel.lock."""
    with _retry_open(_github_request(url, accept="application/json")) as response:
        raw = response.read()
    if hashlib.sha256(raw).hexdigest() != locked_sha256:
        raise ValueError(f"Bazel registry source.json changed since MODULE.bazel.lock: {url}")
    source = json.loads(raw)
    candidates = source.get("urls") or ([source["url"]] if source.get("url") else [])
    matches = [GITHUB_RELEASE_SOURCE.fullmatch(candidate) for candidate in candidates]
    matches = [candidate for candidate in matches if candidate]
    if not matches:
        return None
    if "sha256" in source:
        digest = source["sha256"]
    else:
        integrity = next(
            (item.removeprefix("sha256-") for item in source.get("integrity", "").split()
             if item.startswith("sha256-")),
            None,
        )
        if integrity is None:
            raise ValueError(f"GitHub release archive lacks SHA256 integrity in {url}")
        digest = base64.b64decode(integrity, validate=True).hex()
    if not re.fullmatch(r"[0-9a-fA-F]{64}", digest):
        raise ValueError(f"Invalid Bazel registry SHA256 for {url}")
    match = matches[0]
    return (
        match.group(1), urllib.parse.unquote(match.group(2)),
        urllib.parse.unquote(match.group(3)), digest.lower(),
    )


def prefetch_bazel_release_assets(repo_root: Path, output_dir: Path, *, dry_run: bool = False) -> int:
    """Preload all locked BCR GitHub release assets for Bazel --distdir.

    Bazel normally downloads them through github.com/releases/download, which
    can return 504 even though GitHub's API still serves the same exact bytes.
    The BCR source.json and each tarball are verified against the Git lock.
    """
    lock = json.loads((repo_root / "MODULE.bazel.lock").read_text(encoding="utf-8"))
    sources = sorted(
        (url, checksum) for url, checksum in lock["registryFileHashes"].items()
        if url.startswith("https://bcr.bazel.build/modules/") and url.endswith("/source.json")
    )
    if not sources:
        raise ValueError("No Bazel registry source files pinned in MODULE.bazel.lock")
    with concurrent.futures.ThreadPoolExecutor(max_workers=8) as pool:
        located = list(pool.map(lambda item: bazel_release_asset_from_locked_source(*item), sources))
    archives = [item for item in located if item is not None]
    if not archives:
        raise ValueError("Expected at least one pinned GitHub release archive in the Bazel lockfile")
    basenames: dict[str, str] = {}
    for repo, tag, asset_name, checksum in archives:
        prior = basenames.setdefault(asset_name, checksum)
        if prior != checksum:
            raise ValueError(f"Bazel distdir basename collision: {asset_name}")
    if dry_run:
        for repo, tag, name, digest in archives:
            print(f"PINNED {repo}@{tag}/{name} sha256:{digest}")
        print(f"Reviewed {len(sources)} BCR source.json pins; {len(archives)} GitHub release assets")
        return len(archives)
    output_dir.mkdir(parents=True, exist_ok=True)
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        list(pool.map(
            lambda item: download_release_asset(
                item[0], item[1], item[2], item[3], output_dir / item[2]
            ),
            archives,
        ))
    print(f"Prefetched {len(archives)} pinned Bazel release assets into {output_dir}")
    return len(archives)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="command", required=True)
    fetch = sub.add_parser("download", help="Download exactly one SHA256-pinned GitHub release asset")
    fetch.add_argument("--repo", required=True)
    fetch.add_argument("--tag", required=True)
    fetch.add_argument("--asset", required=True)
    fetch.add_argument("--sha256", required=True)
    fetch.add_argument("--output", type=Path, required=True)
    sub.add_parser("install-dotslash", help="Install native DotSlash for the current Actions runner")
    prefetch = sub.add_parser("prefetch-bazel", help="Prefetch all locked BCR GitHub release assets for Bazel --distdir")
    prefetch.add_argument("--output-dir", type=Path, required=True)
    prefetch.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()

    if args.command == "download":
        download_release_asset(args.repo, args.tag, args.asset, args.sha256, args.output)
    elif args.command == "install-dotslash":
        install_dotslash(
            os.environ["RUNNER_OS"],
            os.environ["RUNNER_ARCH"],
            Path(os.environ["RUNNER_TEMP"]),
            Path(os.environ["GITHUB_PATH"]),
        )
    else:
        prefetch_bazel_release_assets(Path(__file__).resolve().parents[2], args.output_dir, dry_run=args.dry_run)


if __name__ == "__main__":
    main()

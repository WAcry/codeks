#!/usr/bin/env python3
"""Helper for codeks, a personal fork of openai/codex.

The fork model and workflow are described in fork/README.md (Chinese).
Needs Python 3.11+, git 2.32+, and the GitHub CLI (gh) for GitHub tasks.
Uses only the standard library.
"""

from __future__ import annotations

import argparse
import datetime
import json
import os
import re
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass, field
from pathlib import Path
from zoneinfo import ZoneInfo

try:
    import tomllib
except ModuleNotFoundError:
    sys.exit("codeks.py needs Python 3.11 or newer")

TRAILER = "Codeks-Patch"
INFRA_ID = "0000"
FORK_DIR = "fork/"
PATCH_DIR = "fork/patches/"
TEMPLATE = "fork/patches/TEMPLATE.md"
TAG_PREFIX = "codeks/"
UPSTREAM = "upstream"
ORIGIN = "origin"
UPSTREAM_URL = "https://github.com/openai/codex.git"
ORIGIN_URL = "https://github.com/WAcry/codeks.git"
CI_WORKFLOW = "codeks-ci.yml"
NPM_WORKFLOW = "codeks-npm.yml"
WORKFLOW_DIR = ".github/workflows/"
OWN_WORKFLOW_PREFIX = ".github/workflows/codeks-"
MAX_STACK = 200
REPORT_FILE = "codeks-sync-report.md"
REPORT_LIMIT = 40
RANGE_DIFF_LIMIT = 50000
# Private names that must not reach the public repository: one case-insensitive regex per line.
# The list itself is private, so it lives outside the repository. Without it, nothing is scanned.
PRIVATE_TERMS = Path.home() / ".config" / "codeks" / "private-terms"
# Longer than any fence in the patch documents, so a range-diff of them stays inside the block.
REVIEW_FENCE = "`````"
TIMEZONE = ZoneInfo("America/Los_Angeles")
SECTIONS = ("背景", "改动", "必要性", "何时扩展", "何时移除", "冲突处理", "验证", "记录")
META_KEYS = ("watch_paths", "watch_regex", "backport_of", "upstream")
STABLE_TAG = re.compile(r"rust-v(\d+)\.(\d+)\.(\d+)")
DOC_PATH = re.compile(r"fork/patches/(\d{4})-[a-z0-9][a-z0-9-]*\.md")
PATCH_ID = re.compile(r"\d{4}")
SLUG = re.compile(r"[a-z0-9][a-z0-9-]*")
CRATE = re.compile(r"[A-Za-z0-9_-]+")
COMMENT = re.compile(r"<!--.*?-->", re.S)
FENCE = "```"
TOML_BLOCK = re.compile(r"^" + FENCE + r"toml\n(.*?)^" + FENCE, re.S | re.M)


class Fail(Exception):
    """A user-facing error; main() prints it without a traceback."""


# --- process helpers ---------------------------------------------------------


def run(cmd: list[str], *, check: bool = True, input: str | None = None) -> str:
    proc = subprocess.run(cmd, input=input, text=True, capture_output=True)
    if check and proc.returncode != 0:
        detail = (proc.stderr or proc.stdout).strip()
        raise Fail(f"{' '.join(cmd)} 失败：\n{detail}")
    return proc.stdout.rstrip("\n")


def git(*args: str, check: bool = True, input: str | None = None) -> str:
    return run(["git", *args], check=check, input=input)


def git_ok(*args: str) -> bool:
    return subprocess.run(["git", *args], capture_output=True).returncode == 0


def git_live(*args: str, env: dict[str, str] | None = None) -> int:
    """Run git attached to the terminal, for long or interactive steps."""
    return subprocess.run(["git", *args], env=env).returncode


def gh(*args: str, check: bool = True, input: str | None = None) -> str:
    if shutil.which("gh") is None:
        raise Fail("需要已登录的 GitHub CLI（gh）")
    return run(["gh", *args], check=check, input=input)


def maybe_commit(ref: str) -> str | None:
    sha = git("rev-parse", "--verify", "--quiet", f"{ref}^{{commit}}", check=False)
    return sha or None


def commit_of(ref: str) -> str:
    sha = maybe_commit(ref)
    if sha is None:
        raise Fail(f"找不到提交：{ref}")
    return sha


def fetch(remote: str) -> None:
    if git_live("fetch", "--quiet", remote) != 0:
        raise Fail(f"git fetch {remote} 失败")


def is_ancestor(older: str, newer: str) -> bool:
    return git_ok("merge-base", "--is-ancestor", older, newer)


def has_been_at(ref: str, sha: str) -> bool:
    """True if ref (a branch, or HEAD) has pointed at sha, according to its reflog."""
    return sha in git("reflog", "show", "--format=%H", ref, check=False).split()


def operation_in_progress() -> bool:
    names = ("rebase-merge", "rebase-apply", "CHERRY_PICK_HEAD", "MERGE_HEAD")
    return any(Path(git("rev-parse", "--git-path", name)).exists() for name in names)


def ensure_no_operation() -> None:
    if operation_in_progress():
        raise Fail("有未完成的 rebase / cherry-pick / merge，先完成或中止")


def ensure_clean() -> None:
    ensure_no_operation()
    if git("status", "--porcelain", "--untracked-files=no"):
        raise Fail("工作区有未提交的改动，先提交或 git stash")


# --- patch stack -------------------------------------------------------------


@dataclass
class Patch:
    sha: str
    id: str
    subject: str

    @property
    def title(self) -> str:
        return self.subject.removeprefix(f"[codeks {self.id}] ")


@dataclass
class Stack:
    base: str
    patches: list[Patch]

    def find(self, patch_id: str) -> Patch:
        for patch in self.patches:
            if patch.id == patch_id:
                return patch
        raise Fail(f"补丁栈里没有 {patch_id}")


def read_stack(ref: str = "HEAD") -> Stack:
    """Walk first parents from ref; the first commit without a trailer is the base."""
    fmt = f"%H%x1f%P%x1f%(trailers:key={TRAILER},valueonly,separator=%x2c)%x1f%s"
    out = git("log", "--first-parent", f"-n{MAX_STACK + 1}", f"--format={fmt}", ref)
    patches: list[Patch] = []
    for line in out.splitlines():
        sha, parents, ids, subject = line.split("\x1f", 3)
        found = [value.strip() for value in ids.split(",") if value.strip()]
        if not found:
            stray = git("log", "-1", f"--grep=^{TRAILER}:", "--format=%h %s", sha)
            if stray:
                raise Fail(f"{sha[:10]} 不带 {TRAILER} trailer，却位于补丁 {stray} 之上；"
                           "用 codeks.py amend 并入某个补丁，或补上 trailer")
            patches.reverse()
            return Stack(sha, patches)
        if len(parents.split()) != 1:
            raise Fail(f"补丁栈里不能有合并提交：{sha[:10]}")
        if len(found) != 1:
            raise Fail(f"{sha[:10]} 有多个 {TRAILER} trailer")
        patches.append(Patch(sha, found[0], subject))
    raise Fail(f"{ref} 往下 {MAX_STACK} 个提交内没有找到上游基线")


def changed_paths(sha: str) -> list[str]:
    """All paths a commit touches; renames and copies report both sides."""
    fields = git("diff-tree", "--no-commit-id", "-r", "-M", "--name-status", "-z", sha).split("\0")
    paths: list[str] = []
    index = 0
    while index < len(fields) and fields[index]:
        count = 2 if fields[index][0] in "RC" else 1
        paths.extend(fields[index + 1 : index + 1 + count])
        index += 1 + count
    return paths


def version_key(tag: str) -> tuple[int, ...]:
    match = STABLE_TAG.fullmatch(tag)
    return tuple(int(part) for part in match.groups()) if match else (-1,)


def latest_stable() -> str:
    tags = [tag for tag in git("tag", "--list", "rust-v*").split() if STABLE_TAG.fullmatch(tag)]
    if not tags:
        raise Fail("本地没有上游 stable tag，先运行 setup 或 git fetch upstream")
    return max(tags, key=version_key)


def base_name(sha: str) -> str:
    tags = git("tag", "--points-at", sha, "--list", "rust-v*").split()
    if tags:
        stable = [tag for tag in tags if STABLE_TAG.fullmatch(tag)]
        return (stable or sorted(tags))[0]
    if git_ok("merge-base", "--is-ancestor", sha, f"refs/remotes/{UPSTREAM}/main"):
        return f"main-{sha[:10]}"
    return f"g{sha[:10]}"


def resolve_target(target: str) -> tuple[str, str]:
    name = latest_stable() if target == "latest" else target
    return name, commit_of(name)


def diff_id(sha: str) -> str:
    """git patch-id of a commit: equal when two commits make the same change."""
    out = git("patch-id", "--stable", input=git("show", "--format=", sha) + "\n")
    return out.split()[0] if out else ""


def stack_delta(old: Stack, new: Stack) -> list[str]:
    """Patch-level changes from old to new, for tag messages and PR bodies."""
    old_patches = {patch.id: patch for patch in old.patches}
    new_patches = {patch.id: patch for patch in new.patches}
    changes = []
    if old.base != new.base:
        changes.append(f"基线 {base_name(old.base)} → {base_name(new.base)}")
    added = sorted(set(new_patches) - set(old_patches))
    removed = sorted(set(old_patches) - set(new_patches))
    changed = sorted(key for key in set(old_patches) & set(new_patches)
                     if diff_id(old_patches[key].sha) != diff_id(new_patches[key].sha))
    if added:
        changes.append("新增 " + "、".join(added))
    if removed:
        changes.append("移除 " + "、".join(removed))
    if changed:
        changes.append("修改 " + "、".join(changed))
    return changes or ["补丁内容不变"]


# --- patch documents ---------------------------------------------------------


@dataclass
class Doc:
    path: str
    text: str
    title: str = ""
    meta: dict = field(default_factory=dict)
    sections: dict[str, str] = field(default_factory=dict)
    problems: list[str] = field(default_factory=list)

    def meta_list(self, key: str) -> list[str]:
        value = self.meta.get(key, [])
        return value if isinstance(value, list) else []


def parse_doc(path: str, text: str) -> Doc:
    doc = Doc(path, text)
    current: str | None = None
    in_fence = False
    for line in text.splitlines():
        if line.startswith(FENCE):
            in_fence = not in_fence
        elif not in_fence and line.startswith("## "):
            current = line[3:].strip()
            doc.sections[current] = ""
            continue
        elif not in_fence and line.startswith("# ") and not doc.title:
            doc.title = line[2:].strip()
            continue
        if current is not None:
            doc.sections[current] += line + "\n"

    match = TOML_BLOCK.search(text)
    if match is None:
        doc.problems.append("缺少 toml 元数据代码块")
    else:
        try:
            doc.meta = tomllib.loads(match.group(1))
        except tomllib.TOMLDecodeError as err:
            doc.problems.append(f"元数据不是合法的 TOML：{err}")
    for key, value in doc.meta.items():
        if key not in META_KEYS:
            doc.problems.append(f"未知的元数据字段：{key}")
        elif not (isinstance(value, list) and all(isinstance(item, str) for item in value)):
            doc.problems.append(f"元数据 {key} 必须是字符串列表")
    for name in SECTIONS:
        body = doc.sections.get(name)
        if body is None:
            doc.problems.append(f"缺少小节「{name}」")
        elif not COMMENT.sub("", body).strip():
            doc.problems.append(f"小节「{name}」没有内容")
    return doc


def doc_paths(ref: str) -> dict[str, list[str]]:
    """Patch documents in ref, keyed by ID; badly named files use the key ''."""
    found: dict[str, list[str]] = {}
    for path in git("ls-tree", "-r", "--name-only", ref, "--", PATCH_DIR).splitlines():
        if path == TEMPLATE:
            continue
        match = DOC_PATH.fullmatch(path)
        found.setdefault(match.group(1) if match else "", []).append(path)
    return found


def load_doc(ref: str, patch_id: str) -> Doc | None:
    paths = doc_paths(ref).get(patch_id, [])
    if len(paths) != 1:
        return None
    return parse_doc(paths[0], git("show", f"{ref}:{paths[0]}"))


def check_stack(ref: str = "HEAD") -> tuple[Stack, list[str]]:
    stack = read_stack(ref)
    problems: list[str] = []
    if not stack.patches or stack.patches[0].id != INFRA_ID:
        problems.append(f"补丁栈最底层必须是 {INFRA_ID}（fork/ 目录）")
    docs = doc_paths(ref)
    for path in docs.pop("", []):
        problems.append(f"{path}：文件名应为 fork/patches/NNNN-<slug>.md")

    seen: set[str] = set()
    for patch in stack.patches:
        where = f"{patch.id}（{patch.sha[:10]}）"
        if not PATCH_ID.fullmatch(patch.id):
            problems.append(f"{where}：编号必须是四位数字")
            continue
        if patch.id in seen:
            problems.append(f"{where}：编号重复")
            continue
        seen.add(patch.id)
        if not patch.subject.startswith(f"[codeks {patch.id}] "):
            problems.append(f"{where}：提交标题应以「[codeks {patch.id}] 」开头")

        paths = changed_paths(patch.sha)
        fork_paths = [path for path in paths if path.startswith(FORK_DIR)]
        upstream_paths = [path for path in paths if not path.startswith(FORK_DIR)]
        if patch.id == INFRA_ID:
            if upstream_paths:
                problems.append(f"{where}：0000 只能改 fork/，这些改动应放进独立补丁：{', '.join(upstream_paths)}")
            if any(DOC_PATH.fullmatch(path) for path in fork_paths):
                problems.append(f"{where}：补丁文档应和对应补丁在同一个提交里")
            continue

        found = docs.get(patch.id, [])
        if len(found) != 1:
            problems.append(f"{where}：需要且只能有一份文档 fork/patches/{patch.id}-<slug>.md")
            continue
        doc_path = found[0]
        if doc_path not in fork_paths:
            problems.append(f"{where}：文档 {doc_path} 不在这个提交里")
        extra = [path for path in fork_paths if path != doc_path]
        if extra:
            problems.append(f"{where}：补丁只能改自己的文档，其他 fork/ 改动属于 0000：{', '.join(extra)}")
        if not upstream_paths:
            problems.append(f"{where}：没有改上游文件；只改 fork/ 的内容属于 0000")

        doc = parse_doc(doc_path, git("show", f"{ref}:{doc_path}"))
        problems += [f"{doc_path}：{problem}" for problem in doc.problems]
        if not doc.title.startswith(f"{patch.id} "):
            problems.append(f"{doc_path}：一级标题应为「# {patch.id} <标题>」")
        missing = [path for path in upstream_paths if path not in doc.text]
        if missing:
            problems.append(f"{doc_path}：「改动」需要写到这些路径：{', '.join(missing)}")

    for patch_id in sorted(set(docs) - seen):
        problems.append(f"{', '.join(docs[patch_id])}：没有对应的补丁提交")
    if PRIVATE_TERMS.is_file():
        for patch in stack.patches:
            hits = private_hits(added_text(patch.sha))
            if hits:
                problems.append(f"{patch.id}（{patch.sha[:10]}）：补丁内容、提交说明或作者信息含私有名称（{PRIVATE_TERMS}：{'、'.join(hits)}）")
    return stack, problems


def private_hits(text: str) -> list[str]:
    """Return the patterns from PRIVATE_TERMS that match text."""
    if not PRIVATE_TERMS.is_file():
        return []
    patterns = [line.strip() for line in PRIVATE_TERMS.read_text(encoding="utf-8").splitlines()]
    return [p for p in patterns if p and not p.startswith("#") and re.search(p, text, re.IGNORECASE)]


def ensure_public(text: str, what: str) -> None:
    hits = private_hits(text)
    if hits:
        raise Fail(f"{what}含私有名称（{PRIVATE_TERMS}：{'、'.join(hits)}），先改掉")


def added_text(sha: str) -> str:
    """The author, committer and message of a commit, plus the lines that it adds."""
    diff = git("diff", "--no-color", "--unified=0", f"{sha}^", sha)
    added = [line[1:] for line in diff.splitlines() if line.startswith("+")]
    return git("log", "-1", "--format=%an <%ae>%n%cn <%ce>%n%B", sha) + "\n" + "\n".join(added)


def print_problems(problems: list[str]) -> None:
    print("check 未通过：")
    for problem in problems:
        print(f"  - {problem}")


# --- upstream report ---------------------------------------------------------


def upstream_log(rng: str, paths: list[str] | None = None, regex: str | None = None) -> list[str]:
    if paths is not None and not paths:
        return []
    cmd = ["log", "--no-merges", "--date=short", "--format=  - %h %ad %s"]
    if regex is not None:
        cmd.append(f"-G{regex}")
    cmd.append(rng)
    if paths is not None:
        cmd += ["--", *paths]
    out = git(*cmd)
    return out.splitlines() if out else []


def limited(lines: list[str]) -> list[str]:
    if len(lines) <= REPORT_LIMIT:
        return lines
    return lines[:REPORT_LIMIT] + [f"  - ……另有 {len(lines) - REPORT_LIMIT} 个"]


def backport_state(sha: str, target: str, subjects: set[str]) -> str:
    if not git_ok("cat-file", "-e", f"{sha}^{{commit}}"):
        return "本地找不到这个提交，先 git fetch upstream"
    if git_ok("merge-base", "--is-ancestor", sha, target):
        return "新基线已包含 → 删除本补丁"
    if git("log", "-1", "--format=%s", sha) in subjects:
        return "新基线包含同标题的提交（cherry-pick）→ 核对后删除本补丁"
    return "新基线还没有包含，保留"


def build_report(stack: Stack, ref: str, target_name: str, target: str) -> str:
    rng = f"{stack.base}..{target}"
    subjects = set(git("log", "--format=%s", rng).splitlines())
    count = int(git("rev-list", "--count", rng))
    lines = [
        f"<!-- codeks-report {stack.base} {target} -->",
        f"## 上游变化：{base_name(stack.base)} → {target_name}",
        "",
        f"上游新增 {count} 个提交。逐个补丁对照文档的「何时扩展」「何时移除」判断要不要调整。",
    ]
    hits = upstream_log(rng, [FORK_DIR])
    if hits:
        lines += ["", "上游改动了 fork/，同步时会和 0000 冲突：", *limited(hits)]
    added = git("log", "--no-merges", "--diff-filter=A", "--format=", "--name-only", rng, "--", WORKFLOW_DIR)
    if added:
        names = sorted({path for path in added.splitlines() if path})
        lines += ["", "上游新增了 workflow（publish 和 ci 会自动停用）：", *(f"  - {name}" for name in names)]

    for patch in stack.patches:
        if patch.id == INFRA_ID:
            continue
        doc = load_doc(ref, patch.id)
        lines += ["", f"### {doc.title if doc and doc.title else patch.subject}"]
        own = [path for path in changed_paths(patch.sha) if not path.startswith(FORK_DIR)]
        globs = [f":(glob){pattern}" for pattern in (doc.meta_list("watch_paths") if doc else [])]
        total = 0
        hits = upstream_log(rng, own + globs)
        total += len(hits)
        lines.append(f"- 改动本补丁文件或 watch_paths 的上游提交：{len(hits)}")
        lines += limited(hits)
        for pattern in doc.meta_list("watch_regex") if doc else []:
            hits = upstream_log(rng, regex=pattern)
            total += len(hits)
            lines.append(f"- 匹配 watch_regex {pattern!r} 的上游提交：{len(hits)}")
            lines += limited(hits)
        for sha in doc.meta_list("backport_of") if doc else []:
            total += 1
            lines.append(f"- 回移 {sha[:10]}：{backport_state(sha, target, subjects)}")
        if doc is None:
            lines.append("- 找不到补丁文档，先修好再同步")
        elif total == 0:
            lines.append("- 没有相关的上游提交")
    return "\n".join(lines) + "\n"


def report_path() -> Path:
    return Path(git("rev-parse", "--git-path", REPORT_FILE))


def saved_report(base: str) -> str | None:
    """The report saved by sync, if it was made for the current base."""
    path = report_path()
    if not path.exists():
        return None
    first, _, rest = path.read_text(encoding="utf-8").partition("\n")
    parts = first.split()
    return rest if len(parts) >= 4 and parts[3] == base else None


# --- GitHub ------------------------------------------------------------------


def github_repo() -> str | None:
    url = git("remote", "get-url", ORIGIN)
    match = re.search(r"github\.com[:/]([^/]+/[^/]+?)(?:\.git)?/?$", url)
    return match.group(1) if match else None


def require_github_repo() -> str:
    repo = github_repo()
    if repo is None:
        raise Fail(f"origin 不是 GitHub 仓库：{git('remote', 'get-url', ORIGIN)}")
    return repo


def enforce_workflows(repo: str) -> None:
    """Keep only codeks-* workflows active and cancel runs of the others.

    GitHub registers workflow files a few seconds after a push, so first wait
    until every workflow file on origin/main is listed.
    """
    expected = {path for path in git("ls-tree", "--name-only", f"refs/remotes/{ORIGIN}/main", WORKFLOW_DIR).splitlines()
                if path.endswith((".yml", ".yaml"))}
    for attempt in range(13):
        workflows = json.loads(gh("api", f"repos/{repo}/actions/workflows?per_page=100")).get("workflows", [])
        missing = expected - {workflow["path"] for workflow in workflows}
        if not missing:
            break
        time.sleep(5)
    else:
        print(f"注意：GitHub 还没登记这些 workflow，稍后再运行 codeks.py setup --github：{', '.join(sorted(missing))}")
    for workflow in workflows:
        path, state = workflow["path"], workflow["state"]
        if not path.startswith(WORKFLOW_DIR) or state == "deleted":
            continue
        own = path.startswith(OWN_WORKFLOW_PREFIX)
        if own and state != "active":
            gh("api", "-X", "PUT", f"repos/{repo}/actions/workflows/{workflow['id']}/enable")
            print(f"已启用 {path}")
        elif not own and state == "active":
            gh("api", "-X", "PUT", f"repos/{repo}/actions/workflows/{workflow['id']}/disable")
            print(f"已停用上游 workflow {path}")
    for status in ("requested", "queued", "pending", "waiting", "in_progress"):
        runs = json.loads(gh("api", f"repos/{repo}/actions/runs?status={status}&per_page=100"))
        for item in runs.get("workflow_runs", []):
            if not item.get("path", "").startswith(OWN_WORKFLOW_PREFIX):
                gh("api", "-X", "POST", f"repos/{repo}/actions/runs/{item['id']}/cancel", check=False)
                print(f"已取消上游 workflow 运行 {item.get('path')} #{item.get('run_number')}")


def dispatch(repo: str, workflow: str, ref: str, fields: dict[str, str] | None = None) -> None:
    """Start a workflow_dispatch run on ref and print where to follow it."""
    started = datetime.datetime.now(datetime.timezone.utc) - datetime.timedelta(seconds=30)
    cmd = ["workflow", "run", workflow, "--repo", repo, "--ref", ref]
    for key, value in (fields or {}).items():
        cmd += ["-f", f"{key}={value}"]
    gh(*cmd)
    for _ in range(10):
        time.sleep(3)
        runs = json.loads(gh("run", "list", "--repo", repo, "--workflow", workflow, "--event", "workflow_dispatch",
                             "--limit", "5", "--json", "databaseId,url,createdAt"))
        for item in runs:
            if datetime.datetime.fromisoformat(item["createdAt"].replace("Z", "+00:00")) >= started:
                print(f"{workflow} 已触发（{ref}）：{item['url']}")
                print(f"  等待结果：gh run watch {item['databaseId']} --repo {repo} --exit-status")
                return
    print(f"{workflow} 已触发，查看：gh run list --repo {repo} --workflow {workflow}")


def push_branch(branch: str, head: str) -> None:
    """Push a review branch. Refuse to drop remote commits the local branch has never had."""
    ensure_public(branch, "分支名")
    remote = maybe_commit(f"refs/remotes/{ORIGIN}/{branch}")
    if remote == head:
        return
    if remote and not is_ancestor(remote, head) and not has_been_at(f"refs/heads/{branch}", remote):
        raise Fail(f"origin/{branch}（{remote[:10]}）有本地没有的提交，可能是在 GitHub 上提交的修改建议："
                   "先把它们并入对应的补丁，再推送")
    if git_live("push", "--quiet", f"--force-with-lease=refs/heads/{branch}:{remote or ''}", ORIGIN,
                f"{head}:refs/heads/{branch}") != 0:
        raise Fail(f"推送 {branch} 失败")


def finish_review(repo: str, branch: str, head: str, tag: str) -> None:
    """After publish: report how GitHub closed the branch's PR, then delete the branch."""
    prs = json.loads(gh("pr", "list", "--repo", repo, "--head", branch, "--state", "open", "--json", "number,url"))
    for pr in prs:
        number = str(pr["number"])
        # GitHub marks the PR merged once main contains its head; this takes a few seconds.
        state = "OPEN"
        for _ in range(10):
            state = json.loads(gh("pr", "view", number, "--repo", repo, "--json", "state"))["state"]
            if state != "OPEN":
                break
            time.sleep(3)
        if state == "MERGED":
            print(f"PR #{number} 已标为 Merged：{pr['url']}")
        elif state == "OPEN":
            gh("pr", "close", number, "--repo", repo, "--comment", f"已由 codeks.py publish 发布为 {tag}（{head[:10]}）。")
            print(f"PR #{number} 没有被自动标为 Merged，已关闭：{pr['url']}")
    if git_live("push", "--quiet", ORIGIN, "--delete", branch) == 0:
        print(f"已删除远端分支 {branch}")


# --- local main --------------------------------------------------------------


def worktree_of(branch: str) -> str | None:
    path = None
    for line in git("worktree", "list", "--porcelain").splitlines():
        if line.startswith("worktree "):
            path = line[len("worktree "):]
        elif line == f"branch refs/heads/{branch}":
            return path
    return None


def move_local_main(sha: str) -> bool:
    """Point the local main branch at sha without touching uncommitted work."""
    location = worktree_of("main")
    if location is None:
        git("branch", "--force", "main", sha)
    elif os.path.realpath(location) == os.path.realpath(os.getcwd()):
        if commit_of("HEAD") != sha:
            git("reset", "--quiet", "--keep", sha)
    elif git("-C", location, "status", "--porcelain", "--untracked-files=no"):
        print(f"注意：{location} 有未提交的改动，本地 main 没有更新；处理后运行 codeks.py pull")
        return False
    else:
        git("-C", location, "reset", "--quiet", "--keep", sha)
    if maybe_commit(f"refs/remotes/{ORIGIN}/main"):
        git("branch", "--quiet", "--set-upstream-to", f"{ORIGIN}/main", "main")
    return True


# --- commands ----------------------------------------------------------------


def cmd_setup(args: argparse.Namespace) -> int:
    remotes = git("remote").split()
    if ORIGIN not in remotes:
        git("remote", "add", ORIGIN, ORIGIN_URL)
    if UPSTREAM not in remotes:
        git("remote", "add", UPSTREAM, UPSTREAM_URL)
    git("config", "--replace-all", f"remote.{UPSTREAM}.fetch", f"+refs/heads/main:refs/remotes/{UPSTREAM}/main")
    git("config", "--add", f"remote.{UPSTREAM}.fetch", "+refs/tags/rust-v*:refs/tags/rust-v*")
    git("config", f"remote.{UPSTREAM}.tagOpt", "--no-tags")
    tags = f"+refs/tags/{TAG_PREFIX}*:refs/tags/{TAG_PREFIX}*"
    if tags not in git("config", "--get-all", f"remote.{ORIGIN}.fetch", check=False).splitlines():
        git("config", "--add", f"remote.{ORIGIN}.fetch", tags)
    git("config", "rerere.enabled", "true")
    git("config", "rerere.autoUpdate", "true")
    git("config", "pull.ff", "only")
    fetch(UPSTREAM)
    fetch(ORIGIN)
    if maybe_commit(f"refs/remotes/{ORIGIN}/main") and maybe_commit("refs/heads/main"):
        git("branch", "--quiet", "--set-upstream-to", f"{ORIGIN}/main", "main")
    print("本地配置完成：upstream 只拉 main 和 rust-v* tag，origin 拉 codeks/* tag，已开启 rerere 和 pull.ff=only")
    if args.github:
        repo = require_github_repo()
        enforce_workflows(repo)
        gh("api", "-X", "PUT", f"repos/{repo}/actions/permissions", "-F", "enabled=true")
        # A merge commit or a squash would break the patch stack; publish is the normal way to merge.
        gh("api", "-X", "PATCH", f"repos/{repo}", "-F", "allow_merge_commit=false", "-F", "allow_squash_merge=false",
           "-F", "allow_rebase_merge=true")
        print(f"{repo}：Actions 已开启，只有 codeks-* workflow 处于启用状态；PR 只允许 rebase 合并")
    return 0


def cmd_status(args: argparse.Namespace) -> int:
    if args.fetch:
        fetch(UPSTREAM)
        fetch(ORIGIN)
    stack = read_stack()
    base = base_name(stack.base)
    print(f"基线：{base}（{stack.base[:10]}）")
    stable = latest_stable()
    if STABLE_TAG.fullmatch(base) and version_key(stable) > version_key(base):
        print(f"上游有更新的 stable：{stable}（预览 codeks.py report {stable}，同步 codeks.py sync {stable}）")
    else:
        print(f"上游最新 stable：{stable}")
    newest = git("for-each-ref", "--sort=-creatordate", "--count=1", "--format=%(refname:short)", "refs/tags/rust-v*")
    if newest and newest != stable:
        print(f"上游最近创建的 tag（含 alpha）：{newest}")

    print(f"补丁（{len(stack.patches)}）：")
    for patch in stack.patches:
        if patch.id == INFRA_ID:
            stat = "fork/"
        else:
            stat = git("diff", "--shortstat", f"{patch.sha}^", patch.sha, "--", ".", f":(exclude){FORK_DIR}").strip()
        print(f"  {patch.id}  {patch.title}  [{stat}]")

    head = commit_of("HEAD")
    branch = git("branch", "--show-current") or "（detached）"
    origin_main = maybe_commit(f"refs/remotes/{ORIGIN}/main")
    if origin_main is None:
        relation = "origin/main 不存在"
    elif origin_main == head:
        relation = "与 origin/main 相同"
    else:
        relation = f"与 origin/main（{origin_main[:10]}）不同"
    tags = git("tag", "--points-at", head, "--list", f"{TAG_PREFIX}*").split()
    print(f"当前：{branch} {head[:10]}，{relation}，tag：{'、'.join(tags) or '无'}")
    if git("status", "--porcelain", "--untracked-files=no"):
        print("工作区有未提交的改动")
    repo = github_repo()
    if args.fetch and repo and shutil.which("gh"):
        out = gh("run", "list", "--repo", repo, "--workflow", CI_WORKFLOW, "--branch", "main",
                 "--limit", "1", "--json", "status,conclusion,headSha,url", check=False)
        runs = json.loads(out) if out.startswith("[") else []
        if runs:
            last = runs[0]
            print(f"main 最近一次 CI：{last['conclusion'] or last['status']}（{last['headSha'][:10]}）{last['url']}")
    return 0


def cmd_check(args: argparse.Namespace) -> int:
    stack, problems = check_stack(args.rev)
    if problems:
        print_problems(problems)
        return 1
    print(f"check 通过：基线 {base_name(stack.base)}，{len(stack.patches)} 个补丁")
    if git("status", "--porcelain", "--untracked-files=no"):
        print("注意：工作区有未提交的改动，check 只检查已提交的内容")
    return 0


def cmd_new(args: argparse.Namespace) -> int:
    if not SLUG.fullmatch(args.slug):
        raise Fail("slug 只能用小写字母、数字和连字符")
    log = git("log", "--all", f"--grep=^{TRAILER}:", f"--format=%(trailers:key={TRAILER},valueonly)")
    used = set(PATCH_ID.findall(log))
    for path in Path(PATCH_DIR).glob("*.md"):
        match = DOC_PATH.fullmatch(path.as_posix())
        if match:
            used.add(match.group(1))
    next_id = f"{max((int(value) for value in used), default=0) + 1:04d}"
    target = Path(PATCH_DIR) / f"{next_id}-{args.slug}.md"
    text = Path(TEMPLATE).read_text(encoding="utf-8").replace("NNNN", next_id).replace("<标题>", args.title)
    today = datetime.datetime.now(TIMEZONE).date().isoformat()
    text = text.rstrip("\n") + f"\n\n- {today} @ {base_name(read_stack().base)}：新增。\n"
    target.write_text(text, encoding="utf-8")
    print(f"已创建 {target}")
    print("写完代码和文档后提交：")
    print(f'  git add -A && git commit -m "[codeks {next_id}] {args.title}" --trailer "{TRAILER}: {next_id}"')
    return 0


def cmd_amend(args: argparse.Namespace) -> int:
    ensure_no_operation()
    stack = read_stack()
    target = stack.find(args.id)
    if git_ok("diff", "--cached", "--quiet"):
        raise Fail("没有已暂存的改动：先 git add 要并入的内容")
    git("commit", "--quiet", f"--fixup={target.sha}")
    env = dict(os.environ, GIT_SEQUENCE_EDITOR=":")
    if git_live("rebase", "--quiet", "--interactive", "--autosquash", "--autostash", stack.base, env=env) != 0:
        print("rebase 停在冲突上：解决后 git add，再 git rebase --continue；放弃用 git rebase --abort")
        return 1
    print(f"已并入 {target.id}")
    return cmd_check(argparse.Namespace(rev="HEAD"))


def cmd_report(args: argparse.Namespace) -> int:
    if args.fetch:
        fetch(UPSTREAM)
    stack = read_stack()
    name, target = resolve_target(args.target)
    print(build_report(stack, "HEAD", name, target))
    return 0


def cmd_sync(args: argparse.Namespace) -> int:
    ensure_clean()
    if not args.no_fetch:
        fetch(UPSTREAM)
    stack, problems = check_stack()
    if problems:
        print_problems(problems)
        raise Fail("同步前补丁栈就不合规，先修好")
    name, target = resolve_target(args.target)
    if target == stack.base:
        print(f"已经基于 {name}，不需要同步")
        return 0
    head = commit_of("HEAD")
    report = build_report(stack, "HEAD", name, target)
    report_path().write_text(report, encoding="utf-8")
    print(report)
    range_diff = f"git range-diff {stack.base[:12]}..{head[:12]} {target[:12]}..HEAD"
    print(f"开始重放 {len(stack.patches)} 个补丁：{base_name(stack.base)} → {name}")
    print(f"同步前的 HEAD 是 {head[:12]}，需要时可以 git reset --hard {head[:12]} 回到这里")
    if git_live("rebase", "--onto", target, stack.base) != 0:
        print("\n".join([
            "",
            "rebase 停在冲突上，按 fork/SYNC.md「冲突处理」：",
            "  1. git show --stat REBASE_HEAD 看停在哪个补丁，打开它的文档「冲突处理」",
            "  2. 解决后 git add <文件>，再 git rebase --continue",
            "  3. 上游已经实现同样效果：git rebase --skip（补丁和文档一起丢掉）",
            "  4. 放弃本次同步：git rebase --abort",
            f"全部完成后运行 python3 fork/tools/codeks.py check，并用 {range_diff} 审查补丁变化",
        ]))
        return 1
    print("\n补丁变化（= 未变，! 有调整，< 被丢弃）：")
    git_live("range-diff", "--no-color", "-s", f"{stack.base}..{head}", f"{target}..HEAD")
    print(f"查看具体差异：{range_diff}")
    status = cmd_check(argparse.Namespace(rev="HEAD"))
    print("下一步：按报告逐个补丁审查，然后 python3 fork/tools/codeks.py publish")
    return status


def tag_message(tag: str, stack: Stack, note: str | None) -> str:
    lines = [tag, "", f"基线：{base_name(stack.base)}（{stack.base}）", "补丁："]
    lines += [f"  {patch.id} {patch.title}" for patch in stack.patches]
    previous = git("for-each-ref", "--sort=-creatordate", "--count=1", "--format=%(refname:short)",
                   f"refs/tags/{TAG_PREFIX}")
    if previous:
        lines += ["", f"相对 {previous}：" + "；".join(stack_delta(read_stack(previous), stack))]
    if note:
        lines += ["", f"说明：{note}"]
    report = saved_report(stack.base)
    if report:
        lines += ["", report.rstrip()]
    return "\n".join(lines) + "\n"


def cmd_publish(args: argparse.Namespace) -> int:
    ensure_clean()
    stack, problems = check_stack()
    if problems:
        print_problems(problems)
        raise Fail("check 未通过，不能发布")
    head = commit_of("HEAD")
    branch = git("branch", "--show-current")
    here = f"refs/heads/{branch}" if branch else "HEAD"
    fetch(ORIGIN)
    lease = maybe_commit(f"refs/remotes/{ORIGIN}/main") or ""
    # Publishing rewrites main. A branch that has never been at the current origin/main was
    # started before the last publish, and would silently drop what that publish added.
    if lease and not args.force and not is_ancestor(lease, head) and not has_been_at(here, lease):
        tags = "、".join(git("tag", "--points-at", lease, "--list", f"{TAG_PREFIX}*").split()) or "无 tag"
        raise Fail("\n".join([
            f"origin/main（{lease[:10]}，{tags}）是这个分支没有见过的版本：分支创建后 main 又发布过，直接发布会丢掉那些内容。",
            "先把分支更新到最新的 main：只新增了补丁就 git rebase origin/main；改了已有的补丁，就在最新的 main 上重做这些修改。",
            "确认要覆盖时加 --force。",
        ]))
    review = maybe_commit(f"refs/remotes/{ORIGIN}/{branch}") if branch and branch != "main" else None
    if review and review != head and not args.force and not is_ancestor(review, head) \
            and not has_been_at(here, review):
        raise Fail(f"origin/{branch}（{review[:10]}）有本地没有的提交，可能是在 GitHub 上提交的修改建议："
                   "先把它们并入对应的补丁；确认要丢弃时加 --force")
    existing = git("tag", "--points-at", head, "--list", f"{TAG_PREFIX}*").split()
    created = False
    if existing:
        tag = existing[0]
        print(f"HEAD 已经有 tag {tag}")
    else:
        prefix = f"{TAG_PREFIX}{base_name(stack.base)}/"
        numbers = [int(name[len(prefix):]) for name in git("tag", "--list", f"{prefix}*").split()
                   if name[len(prefix):].isdigit()]
        tag = f"{prefix}{max(numbers, default=0) + 1}"
        message = tag_message(tag, stack, args.message)
        ensure_public(message, "tag 说明")
        git("tag", "--annotate", "--cleanup=verbatim", "--file=-", tag, head, input=message)
        created = True
    leases = [f"--force-with-lease=refs/heads/main:{lease}"]
    refspecs = [f"{head}:refs/heads/main", f"refs/tags/{tag}"]
    if review:
        # Point the review branch at the published commit too, so GitHub marks its PR as merged.
        leases.append(f"--force-with-lease=refs/heads/{branch}:{review}")
        refspecs.append(f"{head}:refs/heads/{branch}")
    pushed = git_live("push", "--atomic", *leases, ORIGIN, *refspecs)
    if pushed != 0:
        if created:
            git("tag", "--delete", tag)
        raise Fail("推送失败，新 tag 已撤销。origin/main 可能在别处被更新，先 codeks.py pull 再决定")
    report_path().unlink(missing_ok=True)
    move_local_main(head)
    print(f"已发布 {tag}（{head[:10]}）")
    repo = github_repo()
    if repo is None:
        print("origin 不是 GitHub 仓库，跳过 workflow 检查和 CI")
        return 0
    if review:
        finish_review(repo, branch, head, tag)
    enforce_workflows(repo)
    if not args.no_ci:
        dispatch(repo, CI_WORKFLOW, "main")
    return 0


def cmd_ci(args: argparse.Namespace) -> int:
    repo = require_github_repo()
    branch = args.branch or git("branch", "--show-current")
    if not branch:
        raise Fail("当前不在分支上，用 codeks.py ci <分支> 指定")
    local = maybe_commit(f"refs/heads/{branch}")
    if branch != "main" and local is not None:
        ensure_public(branch, "分支名")
        if git_live("push", "--force", ORIGIN, f"{local}:refs/heads/{branch}") != 0:
            raise Fail(f"推送 {branch} 失败")
    enforce_workflows(repo)
    dispatch(repo, CI_WORKFLOW, branch, {"crates": args.crates} if args.crates else None)
    return 0


def pr_title(old: Stack | None, stack: Stack, branch: str) -> str:
    if old is None:
        return f"[codeks] {branch}"
    if old.base != stack.base:
        return f"[codeks] 同步到 {base_name(stack.base)}"
    delta = stack_delta(old, stack)
    old_ids = {patch.id for patch in old.patches}
    added = [patch for patch in stack.patches if patch.id not in old_ids]
    if len(added) == 1 and delta == [f"新增 {added[0].id}"]:
        return added[0].subject
    return "[codeks] " + "；".join(delta)


def pr_body(repo: str, stack: Stack, head: str, main: str | None, old: Stack | None,
            range_diff: bool = True) -> str:
    lines = ["<!-- Generated by fork/tools/codeks.py pr. Run it again to refresh. -->", "",
             f"- 基线：{base_name(stack.base)}（{stack.base[:10]}）"]
    if old is None or main is None:
        lines.append(f"- 全部 fork 改动：https://github.com/{repo}/compare/{stack.base}..{head}")
    elif old.base != stack.base:
        lines += [
            f"- 相对 main（{main[:10]}）：" + "；".join(stack_delta(old, stack)),
            f"- 新基线上的全部 fork 改动：https://github.com/{repo}/compare/{stack.base}..{head}",
            "- 这是同步 PR：Files changed 和 Commits 里混有上游的变化，主要看下面的 range-diff 和同步报告",
        ]
    else:
        lines += [
            f"- 相对 main（{main[:10]}）：" + "；".join(stack_delta(old, stack)),
            f"- 和 main 的净差异：https://github.com/{repo}/compare/{main}..{head}",
            "- 改的是栈中间的补丁时，Files changed 会多出后面补丁重放的内容，以上面的链接和 range-diff 为准",
        ]
    lines += ["", "审查后不要点 Merge：在这个分支的 worktree 里运行 `python3 fork/tools/codeks.py publish`。"
              "main 会指向这个 PR 的 head，GitHub 会自动把 PR 标为 Merged。"]
    if old is not None and main is not None:
        command = f"git range-diff {old.base[:12]}..{main[:12]} {stack.base[:12]}..{head[:12]}"
        if range_diff:
            diff = git("range-diff", "--no-color", f"{old.base}..{main}", f"{stack.base}..{head}")
            if len(diff) > RANGE_DIFF_LIMIT:
                diff = diff[:RANGE_DIFF_LIMIT] + f"\n……已截断，完整内容在本地运行 {command}"
            lines += ["", "### 补丁变化（= 未变，! 有调整，< 删除，> 新增）", "", f"{REVIEW_FENCE}diff", diff, REVIEW_FENCE]
        else:
            lines += ["", f"补丁变化在本地运行 `{command}` 查看。"]
    report = saved_report(stack.base)
    if report and (old is None or old.base != stack.base):
        lines += ["", report.rstrip()]
    return "\n".join(lines) + "\n"


def cmd_pr(args: argparse.Namespace) -> int:
    repo = require_github_repo()
    ensure_clean()
    stack, problems = check_stack()
    if problems:
        print_problems(problems)
        raise Fail("check 未通过，先修好再开 PR")
    branch = git("branch", "--show-current")
    if not branch or branch == "main":
        raise Fail("在开发分支上运行；main 直接用 publish 发布")
    head = commit_of("HEAD")
    fetch(ORIGIN)
    main = maybe_commit(f"refs/remotes/{ORIGIN}/main")
    old = read_stack(main) if main else None
    body = pr_body(repo, stack, head, main, old)
    if private_hits(body):
        # The range-diff also shows lines that this change removes.
        body = pr_body(repo, stack, head, main, old, range_diff=False)
    title = args.title or pr_title(old, stack, branch)
    ensure_public(f"{title}\n{body}", "PR 标题或描述")
    push_branch(branch, head)
    prs = json.loads(gh("pr", "list", "--repo", repo, "--head", branch, "--state", "open", "--json", "number,url"))
    if prs:
        edit = ["pr", "edit", str(prs[0]["number"]), "--repo", repo, "--body-file", "-"]
        if args.title:
            edit += ["--title", args.title]
        gh(*edit, input=body)
        print(f"已更新 PR：{prs[0]['url']}")
    else:
        create = ["pr", "create", "--repo", repo, "--base", "main", "--head", branch,
                  "--title", title, "--body-file", "-"]
        if not args.ready:
            create.append("--draft")
        print(f"已创建 PR：{gh(*create, input=body)}")
    enforce_workflows(repo)
    if args.ci:
        dispatch(repo, CI_WORKFLOW, branch)
    return 0


def cmd_npm(args: argparse.Namespace) -> int:
    repo = require_github_repo()
    fetch(ORIGIN)
    ref = args.ref
    if ref is None:
        tags = git("tag", "--points-at", f"refs/remotes/{ORIGIN}/main", "--list", f"{TAG_PREFIX}*").split()
        if not tags:
            raise Fail("origin/main 上没有 codeks tag：先 publish，或者指定 tag 或分支")
        ref = tags[-1]
    if maybe_commit(f"refs/tags/{ref}"):
        if not ref.startswith(TAG_PREFIX):
            raise Fail(f"{ref} 不是 codeks tag")
        if not git("ls-remote", "--tags", ORIGIN, f"refs/tags/{ref}"):
            raise Fail(f"tag {ref} 不在 origin 上")
    elif args.publish:
        raise Fail("只有 codeks/* tag 可以发布到 npm")
    elif ref != "main":
        local = maybe_commit(f"refs/heads/{ref}")
        if local is None:
            raise Fail(f"找不到 tag 或本地分支：{ref}")
        push_branch(ref, local)
    enforce_workflows(repo)
    dispatch(repo, NPM_WORKFLOW, ref, {"publish": "true" if args.publish else "false"})
    return 0


def crate_of(path: str) -> str | None:
    parent = Path(path).parent
    while parent.parts and parent.parts[0] == "codex-rs":
        manifest = parent / "Cargo.toml"
        if manifest.is_file():
            data = tomllib.loads(manifest.read_text(encoding="utf-8"))
            return data.get("package", {}).get("name")
        parent = parent.parent
    return None


def cmd_scope(args: argparse.Namespace) -> int:
    """Print the Rust CI scope as key=value lines for GITHUB_OUTPUT."""
    if args.crates.strip():
        crates = args.crates.split()
        bad = [crate for crate in crates if not CRATE.fullmatch(crate)]
        if bad:
            raise Fail(f"crate 名不合法：{', '.join(bad)}")
        rust = True
    else:
        rust = False
        found: set[str] = set()
        for patch in read_stack().patches:
            if patch.id == INFRA_ID:
                continue
            for path in changed_paths(patch.sha):
                if path.startswith("codex-rs/"):
                    rust = True
                    crate = crate_of(path)
                    if crate:
                        found.add(crate)
        crates = sorted(found)
    print(f"rust={'true' if rust else 'false'}")
    print(f"crates={' '.join(crates)}")
    return 0


def cmd_pull(args: argparse.Namespace) -> int:
    ensure_no_operation()
    fetch(ORIGIN)
    fetch(UPSTREAM)
    target = commit_of(f"refs/remotes/{ORIGIN}/main")
    local = maybe_commit("refs/heads/main")
    if local == target:
        print("本地 main 已是最新")
        return 0
    if local is not None and not args.force:
        keep = git("for-each-ref", f"--contains={local}", "--format=%(refname)",
                   f"refs/remotes/{ORIGIN}/", f"refs/tags/{TAG_PREFIX}")
        if not keep:
            raise Fail(f"本地 main（{local[:10]}）有未发布的提交；确认不需要后加 --force")
    if move_local_main(target):
        tags = git("tag", "--points-at", target, "--list", f"{TAG_PREFIX}*").split()
        print(f"本地 main → {target[:10]}（{'、'.join(tags) or '无 tag'}）")
    return 0


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(prog="codeks.py", description="codeks fork 辅助工具，说明见 fork/README.md")
    sub = parser.add_subparsers(dest="command", required=True)

    p = sub.add_parser("setup", help="配置 remote、tag 拉取规则和 rerere（可重复运行）")
    p.add_argument("--github", action="store_true", help="同时开启 Actions 并停用上游 workflow")
    p.set_defaults(func=cmd_setup)

    p = sub.add_parser("status", help="基线、补丁列表和同步状态")
    p.add_argument("--fetch", action="store_true", help="先拉取 upstream 和 origin")
    p.set_defaults(func=cmd_status)

    p = sub.add_parser("check", help="检查补丁栈和补丁文档")
    p.add_argument("rev", nargs="?", default="HEAD")
    p.set_defaults(func=cmd_check)

    p = sub.add_parser("new", help="分配编号并从模板创建补丁文档")
    p.add_argument("slug", help="小写英文和连字符，例如 plaintext-tool-args")
    p.add_argument("title", help="中文标题")
    p.set_defaults(func=cmd_new)

    p = sub.add_parser("amend", help="把已暂存的改动并入某个补丁的提交")
    p.add_argument("id", help="补丁编号，例如 0003")
    p.set_defaults(func=cmd_amend)

    p = sub.add_parser("report", help="只读：列出与各补丁相关的上游提交")
    p.add_argument("target", nargs="?", default="latest", help="上游 tag 或提交，默认最新 stable")
    p.add_argument("--fetch", action="store_true", help="先拉取 upstream")
    p.set_defaults(func=cmd_report)

    p = sub.add_parser("sync", help="把补丁栈 rebase 到新的上游基线")
    p.add_argument("target", nargs="?", default="latest", help="上游 tag 或提交，默认最新 stable")
    p.add_argument("--no-fetch", action="store_true", help="不拉取 upstream")
    p.set_defaults(func=cmd_sync)

    p = sub.add_parser("publish", help="打 tag，推送 main 和 tag，触发 CI")
    p.add_argument("-m", "--message", help="写进 tag 的说明")
    p.add_argument("--no-ci", action="store_true", help="不触发 CI")
    p.add_argument("--force", action="store_true", help="分支没有见过当前的 origin/main 时也发布")
    p.set_defaults(func=cmd_publish)

    p = sub.add_parser("pr", help="推送当前分支，创建或更新用来审查的 draft PR")
    p.add_argument("--title", help="PR 标题，默认按补丁变化生成")
    p.add_argument("--ready", action="store_true", help="创建普通 PR，不用 draft")
    p.add_argument("--ci", action="store_true", help="同时在这个分支上运行 codeks-ci")
    p.set_defaults(func=cmd_pr)

    p = sub.add_parser("ci", help="推送分支并在 GitHub 上运行 codeks-ci")
    p.add_argument("branch", nargs="?", help="分支名，默认当前分支")
    p.add_argument("--crates", default="", help="要测试的 crate，空格分隔；默认取补丁涉及的 crate")
    p.set_defaults(func=cmd_ci)

    p = sub.add_parser("npm", help="在 GitHub 上构建 npm 包，可选发布")
    p.add_argument("ref", nargs="?", help="codeks/* tag（默认 origin/main 上的 tag）；分支只做试构建")
    p.add_argument("--publish", action="store_true", help="构建后发布到 npm（trusted publishing）")
    p.set_defaults(func=cmd_npm)

    p = sub.add_parser("scope", help="（CI 内部使用）输出 Rust 检查范围")
    p.add_argument("--crates", default="", help="手动指定的 crate，空格分隔")
    p.set_defaults(func=cmd_scope)

    p = sub.add_parser("pull", help="把本地 main 更新到 origin/main")
    p.add_argument("--force", action="store_true", help="本地 main 有未发布的提交时也更新")
    p.set_defaults(func=cmd_pull)
    return parser


def main(argv: list[str] | None = None) -> int:
    # Keep our output in order with the git commands we run in between.
    sys.stdout.reconfigure(line_buffering=True)
    args = build_parser().parse_args(argv)
    try:
        os.chdir(git("rev-parse", "--show-toplevel"))
        return args.func(args)
    except Fail as err:
        print(f"错误：{err}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())

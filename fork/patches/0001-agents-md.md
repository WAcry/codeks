# 0001 仓库根 AGENTS.md：让 agent 先读 fork 规范

```toml
watch_paths = ["AGENTS.override.md", "codex-rs/core/src/agents_md.rs"]
watch_regex = []
backport_of = []
upstream = ["https://github.com/openai/codex/pull/49713"]
```

## 背景

在这个仓库里干活的多半是 Codex agent。Codex 会从项目根（有 `.git` 的目录，worktree 也算）到当前目录，把每一级的 `AGENTS.md` 注入为指令，逻辑在 `codex-rs/core/src/agents_md.rs`。agent 不知道 fork 的规范，就会像改普通仓库一样直接改上游文件，也不写补丁文档。

上游原来有自己的根 `AGENTS.md`，在 #49713（2026-09-30）里删掉了，所以现在新增这个文件不会和上游冲突。

仓库是公开的。agent 常从对话和本机环境里得知私有名称，容易把它们写进文档、提交说明或 PR。保密规则本身也在公开的 `AGENTS.md` 里，所以规则只写判断标准，具体名称放在仓库外的清单里，由 `codeks.py` 检查。

## 改动

新增仓库根目录的 `AGENTS.md`（中文）：说明这是一个 fork，列出必读文档（`fork/README.md`、`fork/PATCHING.md`，同步时读 `fork/SYNC.md`），并摘要最关键的规则：公开仓库的保密规则和本机的私有名称清单 `~/.config/codeks/private-terms`、补丁结构、最小改动、本地不编译测试、提交前运行 `check`、审查用 `codeks.py pr` 而发布只用 `publish`。

不改 Codex 的加载逻辑。

实现位置需比较配置、bridge 适配和源码补丁的行为与成本，再与用户确定；配置可行只是依据之一，不自动排除源码补丁。

## 必要性

少了它，agent 不会自动读到 fork 规范，“每一处补丁都要能说清”很快就守不住。规范放在 `fork/` 里不会被自动加载；`AGENTS.override.md` 会让同一目录的 `AGENTS.md` 失效，而且被上游 `.gitignore` 忽略，不适合放进仓库。

少了保密规则，私有名称会随文档、提交说明和 PR 公开出去，而且公开后很难从 git 历史和 GitHub 上彻底清除。

## 何时扩展

- 上游重新加入根 `AGENTS.md`：rebase 会出现 add/add 冲突。保留上游全文，把本补丁的内容作为第一节放在最前面。
- 上游改变项目指令的文件名或加载规则（`codex-rs/core/src/agents_md.rs`）：换成新的文件名或位置。

## 何时移除

- Codex 提供了不往仓库加文件也能注入项目指令的方式，并且同样可靠。
- 不再让 agent 在这个仓库里工作。

## 冲突处理

上游目前没有这个文件，正常不会冲突。冲突只可能来自上游重新加入 `AGENTS.md`，按「何时扩展」处理：合并成一个文件，fork 的内容放在前面。

## 验证

在仓库根目录运行 Codex，`/status` 卡片的 `Agents.md` 一行应包含 `AGENTS.md`；或者问 agent “修改这个仓库要遵守什么规范”，回答应引用 `fork/PATCHING.md`。

## 记录

- 2026-10-15 @ rust-v0.162.1：新增。
- 2026-10-15 @ rust-v0.162.1：加入 PR 审查的规则：不在 GitHub 上合并，发布只用 `publish`。
- 2026-10-15 @ rust-v0.162.1：仓库公开，加入保密规则和本机的私有名称清单。
- 2026-10-20 @ rust-v0.163.0：按用户要求，将“优先配置”调整为比较实现位置及成本后与用户确定；已有明确选择不重复确认。

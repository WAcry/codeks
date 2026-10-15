# 0002 删除 Dependabot 配置

```toml
watch_paths = [".github/dependabot.yml", "renovate.json", ".github/renovate.json"]
watch_regex = []
backport_of = []
upstream = []
```

## 背景

上游的 `.github/dependabot.yaml` 每周检查 bun、cargo、docker、GitHub Actions 和 Rust 工具链的更新，并自动开 PR。codeks 是独立的私有仓库，不是 GitHub fork（公开仓库的 fork 不能设为私有）。按 GitHub 文档，非 fork 仓库的默认分支上只要有这个配置文件就会启用版本更新，彻底关闭的方法是删除这个文件；仓库设置里没有对应的开关。

## 改动

删除 `.github/dependabot.yaml`。不涉及代码。

## 必要性

少了它，Dependabot 每周会在 codeks 上开一批依赖升级 PR（cargo 覆盖 `codex-rs` 下的所有 crate），带来通知噪音。这些 PR 一旦合入，会在 `Cargo.lock` 这类最热的文件上制造与上游无关的分叉，违背“只跟随上游”。依赖升级随上游同步获得就够了。

## 何时扩展

上游新增其他会让 GitHub 在本仓库自动行动的依赖更新配置，例如改名为 `.github/dependabot.yml`，或加入 Renovate 的 `renovate.json`：一并删除，并把路径补进本文档。

## 何时移除

- GitHub 提供了关闭版本更新的仓库级开关：改用开关，删掉本补丁。
- codeks 变成了 GitHub fork（fork 默认不启用版本更新）。

## 冲突处理

上游修改这个文件时，rebase 会报 modify/delete 冲突：执行 `git rm .github/dependabot.yaml`，再 `git rebase --continue`。

## 验证

`git ls-files .github/dependabot.yaml` 没有输出；GitHub 上 codeks 没有 Dependabot 开的 PR。

## 记录

- 2026-10-15 @ rust-v0.162.1：新增。

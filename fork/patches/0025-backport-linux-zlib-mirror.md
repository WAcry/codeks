# 0025 回移：优先从 Launchpad 下载 Linux zlib

```toml
watch_paths = ["MODULE.bazel", "MODULE.bazel.lock", "patches/BUILD.bazel"]
watch_regex = ["rules_rs_zlib_snapshot_urls|snapshot.ubuntu.com|zlib1g_"]
backport_of = ["ede589336e9ecc64cb05045c2961fb1277858f1d"]
upstream = ["https://github.com/openai/codex/pull/59448", "https://github.com/hermeticbuild/rules_rs/commit/ee9a99d0a8e7784b3104dd188123750443cf2c26"]
ci_skip = []
```

## 背景

`rust-v0.167.1` 的 `rules_rs` 补丁只从 Ubuntu snapshot 下载固定的 Linux zlib 包。六平台发布构建中，Linux ARM64 与 x64 Voice 在下载该输入时连续遇到 HTTP 502/503，重试又遇到域名解析失败，尚未进入相关编译。普通 Ubuntu archive 与 ports 已不再提供这个旧版本，不能简单恢复原 URL。

上游已在 [Prefer Launchpad for Linux zlib downloads in rules_rs](https://github.com/openai/codex/commit/ede589336e9ecc64cb05045c2961fb1277858f1d) 增加同一包的 Launchpad 官方入口。该修复不在固定基线中，需要回移以恢复发布，不等待下一次基线升级。

## 改动

原样应用上游提交对 `patches/rules_rs_zlib_snapshot_urls.patch` 的差异：Linux ARM64 与 x64 优先从 `launchpad.net/ubuntu/+archive/primary/+files/` 下载；原有 `snapshot.ubuntu.com` 日期 URL 保留为第二入口。

两种架构仍使用 `zlib1g_1.3.dfsg-3.1ubuntu2.1` 和原有 SHA256。Bazel 的 `download_and_extract` 原生接受 URL 列表，继续校验下载内容。没有升级 `rules_rs`、改变运行时、增加下载器或跳过测试。

## 必要性

这是固定构建输入的下载故障，与产品配置或运行时逻辑无关。反复重跑只能等待单一来源恢复；0015 的预取器只覆盖 BCR 元数据中的 GitHub Release 资产，扩展它来处理 Ubuntu 包反而增加维护面。用户已选择原样回移上游对实际下载点的最小修复，保留版本、摘要与发布来源校验。

## 何时扩展

若上游调整 `rules_rs` 版本、Linux zlib 来源或包版本，先检查新基线是否已经采用等价修复。其他依赖下载问题单独判断，不扩大本回移为通用镜像机制。

## 何时移除

新基线包含 `ede589336e9ecc64cb05045c2961fb1277858f1d`、其原始提交 `2582fb2db50eb8fd652ffd02b11e226d106561d4`，或升级后的 `rules_rs` 已直接支持等价官方下载入口时，删除整个 0025 补丁。

## 冲突处理

以新基线固定的包版本和 SHA256 为准，不保留已过时的下载包。若仍需要回移，只应用上游对应的 URL 修复；已有等价行为时删除补丁，不叠加备用入口。

## 验证

本地比较修改后的文件与上游提交的 blob，检查版本和 SHA256 不变，运行 `git diff --check` 与 `codeks.py check`。未在本地编译或运行测试。当前开发环境访问 Launchpad 仍遇到解析或连接问题，未取得包字节，不能将此调查视为下载成功验证。

远端六平台发布应照常执行，Linux ARM64 与 x64 Voice 必须通过 Bazel 下载、SHA256 校验和原有构建；发布产物继续遵守源提交一致性检查，不复用其他提交的完整二进制产物。

## 记录

- 2026-11-11 @ rust-v0.167.1：原样回移上游 Launchpad 下载入口，保留 Linux zlib 版本、摘要与完整构建流程。

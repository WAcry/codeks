# 0003 fork CI：codeks-ci workflow

```toml
watch_paths = [
  ".github/actions/setup-ci/**",
  ".github/actions/setup-rusty-v8/**",
  ".github/workflows/rust-ci-full-nextest-platform.yml",
  "codex-rs/rust-toolchain.toml",
  "codex-rs/.config/nextest.toml",
]
watch_regex = []
backport_of = []
upstream = []
```

## 背景

本地默认不编译、不测试，fork 的编译和测试放到 GitHub Actions 上做。上游的 workflow 是为 openai/codex 写的：推送 `main` 会触发 `blocking-ci` 和 `postmerge-ci`，它们依赖上游自己的 runner，在 codeks 里只会排队或失败。所以 codeks 在 GitHub 上停用了全部上游 workflow（见 README「GitHub 仓库设置」），只运行 fork 自己的 workflow。

一次带 Rust 的运行要二三十分钟，标准 runner 的磁盘只有 14 GB。所以 CI 只在需要时运行，而且只测补丁涉及的部分。

## 改动

新增 `.github/workflows/codeks-ci.yml`，只能手动触发（`workflow_dispatch`）：`codeks.py publish` 推送后自动触发，`codeks.py ci` 在开发分支上触发。同一分支上的新运行会取消旧运行。

- `check` job（约 1 分钟）：浅克隆最近 100 个提交，运行 `codeks.py check`；再用 `codeks.py scope` 决定 Rust 范围：补丁改了 `codex-rs/` 就运行 Rust job，要测试的 crate 取补丁改动所在的 crate。手动触发时可以用输入 `crates` 指定。
- `rust` job：先清理 runner 磁盘；复用上游的 `./.github/actions/setup-ci`（DotSlash、just、user namespace、构建目录）和 `./.github/actions/setup-rusty-v8`（预编译 V8）；安装和上游一样的系统依赖；`cargo fmt --check`；构建 `codex`、`codex-code-mode-host`、`codex-linux-sandbox`，以及 codex-rmcp-client 的 MCP 测试服务器 `test_stdio_server`、`test_streamable_http_server`，这一步同时确认整个 CLI 能编译，测试也要用到这些二进制；最后 `cargo nextest run -p <crate>…`。关闭 debuginfo 和增量编译来减少磁盘和时间；用 sccache 加 actions/cache 在运行之间复用编译结果。

  上游 CI 把整个 workspace 打成一个 nextest 归档，所有二进制都在里面。我们只测补丁涉及的 crate，`nextest run -p <crate>` 不会编译别的 crate 的二进制，所以测试要用的辅助二进制都要在这一步列出来。

workflow 只负责调度，判断逻辑在 `fork/tools/codeks.py`（0000）里，所以本补丁很少需要改。

## 必要性

少了它，fork 的改动只能在本地编译测试，违背“本地默认不编译测试”，或者干脆没有测试。workflow 文件只能放在 `.github/workflows/`，不能放进 `fork/`。它是新文件，上游没有同名文件，不会冲突。

## 何时扩展

- 上游的测试需要新的系统依赖或辅助二进制：对照 `rust-ci-full-nextest-platform.yml` 里的 apt 包和 `cargo build` 步骤补上。测试报 `CargoBinError::NotFound` 时，把缺的二进制加进 “Build CLI and test helpers” 这一步。
- 上游改了 `setup-ci` 或 `setup-rusty-v8` 的名字、输入或行为：跟着改调用方式。
- 上游升级 Rust 工具链（`codex-rs/rust-toolchain.toml`）：把 `dtolnay/rust-toolchain` 的版本和固定 SHA 改成上游 workflow 里的新值。

## 何时移除

不再需要远端 CI，或者改到别处运行（自托管 runner、其他仓库）。

## 冲突处理

新文件，正常不会冲突。如果上游新增了同名文件，就把我们的文件改名为另一个 `codeks-*.yml`。

## 验证

- `publish` 打印的运行地址里，`check` job 通过；有 Rust 补丁时，`rust` job 也通过。
- `gh workflow list --all --repo WAcry/codeks` 里，上游 workflow 都是 disabled，只有 `codeks-ci` 是 active。

## 记录

- 2026-10-15 @ rust-v0.162.1：新增。
- 2026-10-15 @ rust-v0.162.1：补上 MCP 测试服务器 `test_stdio_server` 和 `test_streamable_http_server`。第一次测试 codex-core 时缺了它们，152 个 MCP 相关测试失败。

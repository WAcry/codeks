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

- 版本号：编译前把 `codex-rs/Cargo.toml` 里 `[workspace.package]` 的 `version` 改回 `0.0.0`，然后确认改成功了；上游改了这一行的写法时，这一步会失败。只改 CI 的工作目录，不提交。
- 跳过的测试：`codeks.py scope` 收集补丁文档元数据里的 `ci_skip`（见 PATCHING.md），合成一个 nextest 过滤表达式 `not ((…) | (…))`，测试时用 `-E` 传入。没有 `ci_skip` 时不加过滤。

workflow 只负责调度，判断逻辑在 `fork/tools/codeks.py`（0000）里，所以本补丁很少需要改。

## 必要性

少了它，fork 的改动只能在本地编译测试，违背“本地默认不编译测试”，或者干脆没有测试。workflow 文件只能放在 `.github/workflows/`，不能放进 `fork/`。它是新文件，上游没有同名文件，不会冲突。

版本号：上游 main 的工作区版本一直是 `0.0.0`，上游的测试都在这个版本下运行；发布 tag 只把这一行改成真实版本，`Cargo.lock` 里仍是 `0.0.0`。有些测试依赖它：codex-tui 的快照里写的是 `v0.0.0`，codex-cli 的 `stable_version()` 把 `0.0.0` 当作开发版、不开启自动更新。在发布 tag 上测试这两个 crate，有 37 个和补丁无关的失败。CI 编译出的二进制不分发，版本号不影响别的东西；npm 包（0004）照常用真实版本。

跳过的测试：补丁有意改掉某个行为后，验证这个行为的上游测试一定会失败。改上游测试文件会增加冲突，所以把它们写进补丁自己文档的 `ci_skip`。删掉补丁时，跳过也一起消失。

## 何时扩展

- 上游的测试需要新的系统依赖或辅助二进制：对照 `rust-ci-full-nextest-platform.yml` 里的 apt 包和 `cargo build` 步骤补上。测试报 `CargoBinError::NotFound` 时，把缺的二进制加进 “Build CLI and test helpers” 这一步。
- 上游改了 `setup-ci` 或 `setup-rusty-v8` 的名字、输入或行为：跟着改调用方式。
- 上游升级 Rust 工具链（`codex-rs/rust-toolchain.toml`）：把 `dtolnay/rust-toolchain` 的版本和固定 SHA 改成上游 workflow 里的新值。
- “Use the upstream development version” 这一步失败：上游改了 `[workspace.package]` 里版本的写法，按新写法修改 `sed`。

## 何时移除

不再需要远端 CI，或者改到别处运行（自托管 runner、其他仓库）。

改版本号这一步：上游的测试不再依赖 `0.0.0`，或者 fork 改为基于上游 main，而不是发布 tag。

## 冲突处理

新文件，正常不会冲突。如果上游新增了同名文件，就把我们的文件改名为另一个 `codeks-*.yml`。

## 验证

- `publish` 打印的运行地址里，`check` job 通过；有 Rust 补丁时，`rust` job 也通过。
- `gh workflow list --all --repo WAcry/codeks` 里，上游 workflow 都是 disabled，只有 `codeks-ci` 是 active。
- 有 `ci_skip` 时，nextest 的结果里有对应数量的 skipped。

## 记录

- 2026-10-15 @ rust-v0.162.1：新增。
- 2026-10-15 @ rust-v0.162.1：补上 MCP 测试服务器 `test_stdio_server` 和 `test_streamable_http_server`。第一次测试 codex-core 时缺了它们，152 个 MCP 相关测试失败。
- 2026-10-16 @ rust-v0.162.1：编译和测试时把工作区版本改回 `0.0.0`，修复 codex-tui 和 codex-cli 的 37 个版本相关失败；按补丁文档的 `ci_skip` 跳过测试。

- 2026-10-23 @ rust-v0.164.0：Rust 工具链与上游同步至 1.99.1，并使用上游固定的安装 Action SHA；V8 继续由上游 setup-rusty-v8 按当前依赖准备。

# 0034 Fork CI/CD 构建与发布

```toml
watch_paths = [
  ".github/actions/setup-ci/**",
  ".github/actions/setup-rusty-v8/**",
  ".github/actions/setup-msvc-env/**",
  ".github/workflows/rust-ci-full-nextest-platform.yml",
  ".github/workflows/rust-release*.yml",
  ".github/scripts/install-musl-build-tools.sh",
  ".github/scripts/install-musl-openssl.sh",
  ".github/scripts/setup-voice-windows.ps1",
  ".github/scripts/voice-cygwin*",
  ".github/scripts/voice_windows_tools.py",
  ".github/scripts/compute-bazel-windows-path.ps1",
  ".github/scripts/run-bazel-ci.sh",
  ".github/scripts/run_bazel.py",
  ".github/scripts/watch_voice_bazel.py",
  ".github/scripts/rusty_v8_bazel.py",
  "codex-cli/**",
  "codex-rs/rust-toolchain.toml",
  "codex-rs/.config/nextest.toml",
  "codex-rs/exec-server-cli/**",
  "codex-rs/exec-server/**",
  "codex-rs/linux-sandbox/**",
  "codex-rs/voice-host/**",
  "codex-rs/realtime-webrtc/**",
  "scripts/build_codex_package.py",
  "scripts/codex_package/**",
  "scripts/workspace-status.*",
  "third_party/v8/**",
  "third_party/voice/**",
  "MODULE.bazel*",
  ".bazelversion",
  ".bazelrc",
  "fork/tools/build_voice_windows.ps1",
  "fork/tools/npm_package.py",
]
watch_regex = [
  "rusty.v8|setup-rusty-v8|rusty-v8-v",
  "github.com/.*/releases/download/",
  "rules_cc|install-dotslash|--distdir|--disk_cache",
  "codeks-(base|voice|exec)|workflow_dispatch|actions/download-artifact",
  "needs[.]|if:.*always|Publish.*release|Verify.*target",
]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

上游工作流使用自己的 runner、签名和发布环境。Codeks 在公开 GitHub Actions runner 上验证补丁栈，并用同一提交的二进制发布六个平台的 npm 和 GitHub Release 包。此前 CI 同时测试多个大型 crate 时 runner 失联。发布流程曾遇到官方资产下载 504、打包语法错误及恢复时的 smoke job 被跳过。

## 改动

### 补丁验证

- `.github/workflows/codeks-ci.yml` 由 `codeks.py publish` 或 `codeks.py ci` 手动调度，同一 ref 上的新运行取消旧运行。检查补丁栈和下载器，再按 `codeks.py scope` 得出的 crate 矩阵运行。core、app-server 和 TUI 各用独立 runner，其余每组最多四个 crate，矩阵最多同时运行三组。Cargo 和 nextest 并发各为 2，每组独立保存 sccache。用当前基线的 Rust 1.99.1 和上游同一固定 Action SHA。
- 每组清理磁盘、准备 Linux 构建依赖，运行格式检查，构建 `codex`、`codex-code-mode-host`、`codex-exec-server`、`codex-linux-sandbox`、`codex-message-board-mcp`、`test_stdio_server` 和 `test_streamable_http_server`，再运行该组的 nextest。只在测试工作目录把 workspace 版本设为上游开发版 `0.0.0`；发行包使用真实版本。
- nextest 只跳过各补丁文档明确登记的 `ci_skip` 用例。失败后保存 Insta 的 `.snap.new` 七天供审阅，按实际输出和权限语义核对候选快照。每个测试分组都必须通过。

### 原生构建和发行

- `.github/workflows/codeks-npm.yml` 由 `codeks.py npm` 调度。prepare 检查补丁栈、版本、矩阵和内嵌组装脚本语法，再分别构建 CLI 和 voice。Linux、macOS、Windows 的 x64 和 ARM64 均使用对应架构 runner。Linux CLI 采用 musl，保留上游 bwrap 摘要校验；voice 采用 GNU ABI。macOS voice 最低版本 14.0，保留并验证临时签名。Windows 使用 MSVC、长路径和简短的 voice 临时目录。 `fork/tools/build_voice_windows.ps1` 将已校验的 SystemRoot 和实际宿主架构交给上游 Bazel 包装脚本，由包装脚本配置 MSVC 和语音依赖。缓存 Cargo 下载及 Bazel 依赖和本地产物。
- pack 使用同一源码提交的原生二进制、voice helper 与 runtime，沿用上游布局、许可文件、runtime 清单和摘要。生成 `@wacry/codeks` 主包及六个平台可选依赖，并生成 GitHub Release 的完整 CLI 归档与 SHA256。完整 CLI 包带独立 executor；npm 包沿用上游布局，通过 `codex exec-server` 启动。版本为 `<基线版本>-codeks.<n>`，二进制显示上游版本，npm 元数据记录 fork 与上游的准确来源。
- `.github/scripts/codeks_exec_release.py` 从同提交完整包提取独立 Exec Server、相应平台沙箱辅助程序和 rg，保留布局、权限和签名。只在 GitHub Release 提供此独立归档和 SHA256。 `.github/scripts/test_codeks_exec_release.py` 验证必需资源和错误处理。
- 六个平台原生 smoke 核对最终包、版本、voice 初始化和关闭、独立 executor 的摘要、布局及适用的签名。`fork/tools/release_smoke.py` 通过 stdio 验证 npm 启动器的 `codex exec-server` 和独立 executor 都能完成 JSON-RPC 初始化、返回正确版本并退出。只有显式 `publish=true`、有效 fork tag 和六组 smoke 均通过才用 npm OIDC 与 GitHub Release 发布。finalize 核对 prepare、pack、smoke 及请求发布时的 publish 均实际成功。
- 可用 `reuse_build_run` 和 `reuse_build_tag` 恢复原 tag 的打包或发布。核对 run 属于同一仓库、源码 SHA 与原 tag 完全相同，且 12 个 CLI/voice 原生产物齐备、非空、未过期；组装、验证和发布均继续用原 tag。恢复只跳过原生构建，所有组装和原生 smoke 仍执行。默认 run ID 为 0 时只取当前运行产物。

### 固定构建输入

- `.github/actions/setup-rusty-v8/action.yml` 对浏览器下载入口有限重试，必要时通过 GitHub 官方资产 API 按 tag 与名称取得相同文件。仍校验源码固定的 manifest 摘要、预编译库和 bindings。
- `.github/scripts/codeks_release_asset.py` 使用原子写入、资产大小与固定 SHA256 校验；只重试暂时失败，跨域重定向删除 Authorization。`.github/actions/setup-ci/action.yml` 和发布工作流使用它准备固定的 DotSlash，`.github/scripts/install-musl-openssl.sh` 使用它获取固定的 OpenSSL。
- Bazel 预取按 `MODULE.bazel.lock` 校验 BCR `source.json` 与源码归档，再将 GitHub Release 资产放入 `--distdir`。Windows 与其他平台使用同一输入校验路径。CI 和发布准备阶段执行 `.github/scripts/test_codeks_release_asset.py` 的离线验证。
- `.github/scripts/run_bazel.py` 优先保留调用方显式提供的本地 `--disk_cache`，让 fork 的 GitHub runner 可在无 GCS 凭据时复用本地缓存；上游未显式提供本地 cache 的默认行为继续沿用。

## 必要性

两个工作流共享构建工具、预编译资产和版本约束。合在一处可随上游同步检查 CI 与发行是否使用相同构建输入。测试矩阵隔离重型 crate 的资源消耗，发布验证覆盖实际下载包。官方 API 回退复用相同固定资产的校验，恢复模式则复用已完成且可核实来源的原生产物，可分别解决下载失败和组装中断。

## 何时扩展

跟随上游 Rust、V8、系统依赖、辅助二进制和打包资源变化。构建目标、Bazel 锁文件、原生 artifact 命名、打包布局或发布 job 有变化时，同时检查下载完整性、同提交约束和全部目标的 smoke。新增必需 job 时把其真实结果纳入最终检查。

## 何时移除

有提供等价测试覆盖的 CI、等价的可信构建输入下载，或带同提交来源核验及原生 smoke 的发布机制时，可逐段删除相应适配。若测试不再依赖开发版版本号，则删除 CI 中的版本改写。

## 冲突处理

保留上游版本、目标和签名/摘要规则，按新源码更新最小挂载点。继续使用 fork 自己的两个工作流。确认 `pack` 只使用本次成功构建或已核验的同 tag 产物，必需 job 的 skipped 和 failure 均使流程失败。

## 验证

- 运行 `codeks.py check` 与两个工作流的 `actionlint`，确认内嵌 Python 可解析。检查固定资产测试覆盖 SHA 不符、BCR 锁定、临时错误和重定向。
- 确认发布候选提交的 `codeks-ci` 各分组均实际通过。核对 GitHub 上仅启用 fork 管理的工作流。
- 对准确 tag 执行发布流程，确认全部原生构建、打包、六组 smoke 和 finalize 通过，核对 npm 版本、平台包以及 GitHub Release 的完整 CLI、独立 Exec Server 归档和 SHA256。恢复流程可先用 `publish=false` 验证只跳过原生构建的路径。
- 在支持各自沙箱的主机上，用 npm 启动器和独立 executor 验证普通工作区写入以及只读策略拒绝写入。Linux 清除宿主 `PATH` 中的 `bwrap`，以覆盖包内的辅助程序和摘要校验。六平台 stdio smoke 验证启动、协议和退出。

## 记录

- 2026-10 至 2026-11 @ rust-v0.162.1 至 rust-v0.170.0：新增 CI、六平台发行与独立 executor；补齐固定构建资产 API 回退、同提交恢复和发布门禁；CI 改用分组 runner。
- 2026-11-20 @ rust-v0.171.0：合并原 0003、0004、0015、0017；同步 Rust 1.99.1，补齐消息看板测试程序、失败快照产物和显式 Bazel 本地缓存。
- 2026-11-20 @ rust-v0.171.0：Windows x64 和 ARM64 voice 作业均在启动 Bazel 前因缺少 `VOICE_WINDOWS_SYSTEM_ROOT` 退出。构建脚本补齐上游包装器要求的 SystemRoot 和宿主架构环境变量。
- 2026-11-20 @ rust-v0.171.0：两种 Windows 架构进入 Bazel 后，旧的 `@local_config_cc` 工具链覆盖使分析失败。移除这个覆盖及重复的原生语音参数，复用上游 Windows MSVC 主机平台和语音工具链配置。
- 2026-11-20 @ rust-v0.171.0：Linux x64 安装 musl 工具时，OpenSSL 资产的 GitHub REST 请求因匿名配额返回 403。仅为该步骤传入 Actions 自带的只读 token，沿用下载器对跨域跳转移除鉴权和固定摘要校验的逻辑。
- 2026-11-24 @ rust-v0.172.0：六平台 npm smoke 增加 `codex exec-server` 的 stdio 初始化、版本和退出检查，与独立 executor 共用验证逻辑。

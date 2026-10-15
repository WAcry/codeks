# 0004 fork npm 包：codeks-npm workflow

```toml
watch_paths = [
  ".github/workflows/rust-release*.yml",
  ".github/scripts/install-musl-build-tools.sh",
  ".github/scripts/install-musl-openssl.sh",
  ".github/scripts/setup-voice-windows.ps1",
  ".github/scripts/voice-cygwin*",
  ".github/scripts/voice_windows_tools.py",
  ".github/scripts/compute-bazel-windows-path.ps1",
  ".github/scripts/run-bazel-ci.sh",
  ".github/scripts/run_bazel_with_buildbuddy.py",
  ".github/scripts/watch_voice_bazel.py",
  ".github/actions/setup-rusty-v8/**",
  ".github/actions/setup-msvc-env/**",
  "codex-cli/**",
  "scripts/build_codex_package.py",
  "scripts/codex_package/**",
  "scripts/workspace-status.*",
  "third_party/voice/**",
  "codex-rs/voice-host/**",
  "codex-rs/realtime-webrtc/**",
  "codex-rs/rust-toolchain.toml",
  "MODULE.bazel*",
  ".bazelversion",
  ".bazelrc",
]
watch_regex = []
backport_of = []
upstream = []
```

## 背景

其他个人项目通过 npm 依赖 `@openai/codex` 来使用 Codex。fork 需要保持相同的安装和运行布局，并按用户需求提供 Linux、macOS、Windows 的 x64 / ARM64 包及 TUI voice runtime。完整下载包也放到 GitHub Release，方便不使用 npm 的环境直接部署。

上游先构建原生程序和语音运行时，再用 `scripts/build_codex_package.py` 组装 `bin/`、`codex-resources/`、`codex-path/`。`codex-cli/scripts/build_npm_package.py` 随后生成主包和平台包：平台包使用同一包名的不同版本，主包通过 `optionalDependencies` 别名选择当前平台。

上游 release workflow 依赖自己的 runner、签名和发布环境，不能直接用于 fork。我们保持其源码、包布局和构建脚本，单独编排公开 runner 上的构建及 fork 发布，不修改上游 workflow。

## 改动

新增 `.github/workflows/codeks-npm.yml`，只支持手动触发，由 `codeks.py npm` 启动。这个 workflow 属于 0004；它调用的 `fork/tools/npm_package.py`、`voice_package.py`、`build_voice_windows.ps1` 和 `release_smoke.py` 属于 fork 工具层 0000。上游文件只增加此 workflow，构建和打包差异集中在这些适配脚本。

流程为 `prepare → CLI/voice 构建 → pack → smoke → publish`：

1. **prepare**：运行 `codeks.py check`，解析 npm 版本，并生成六个平台的矩阵。要求发布但当前 ref 不是有效的 codeks tag 时立即失败。
2. **binaries**：六个原生 runner 分别构建 CLI 和必需资源。Linux x64 / ARM64 使用 Ubuntu runner，目标为 musl；macOS x64 / ARM64 使用对应标准 macOS runner，目标为 Darwin；Windows x64 / ARM64 使用对应 Windows runner，目标为 MSVC。
3. **voice**：另六个原生 job 使用同一提交构建 helper 和 native runtime，与 CLI job 分开，降低单个 runner 的磁盘和内存峰值。构建结果先 stage、seal，再归档传给汇总 job。
4. **pack**：在 Linux 汇总所有 base package 和 voice 产物，逐平台组装完整包，生成 Linux/macOS `.tar.gz`、Windows `.zip` 和对应 `.sha256`；再校验、收集六个平台，统一生成 npm 主包和平台包。
5. **smoke**：在六个对应原生 runner 上验证最终 npm 产物。全部成功才允许发布。
6. **publish**：仅在输入 `publish=true` 时运行；同一个 job 发布 npm 包，再创建或更新当前 tag 的 GitHub Release 下载产物。

### CLI 和资源

Linux 延续上游 musl 构建方法：Rust 1.95.0、Zig 0.14.0、`install-musl-build-tools.sh`、关闭 aws-lc jitter entropy，并通过 `setup-rusty-v8` 校验和准备 V8。先构建并 strip `bwrap`，将其 SHA256 作为 `CODEX_BWRAP_SHA256` 编进 CLI，最后保持该文件原样打包。构建 `codex`、`codex-code-mode-host`、`codex-exec-server` 后 strip，规范打包器负责补齐 rg；三个预编译程序都显式传给打包器，避免组装时再次启动 Cargo。

macOS 使用 `macos-15-intel`（x64）和 `macos-15`（ARM64）原生构建，不使用收费的 larger runner。这两种标准 runner 在公开仓库免费可用（[GitHub 官方说明](https://docs.github.com/en/actions/reference/runners/github-hosted-runners)）。Cargo 和 Bazel 构建并发设为 2，适应标准 ARM runner 的内存；保留 CLI/voice 分 job。CLI、Code Mode host、executor 在 strip 后打包，包内四个程序（包括 rg）统一使用本机 ad-hoc 签名并验证，然后才归档。

基础包的参数数组以共有的 `--package-dir` 初始化，保证 macOS 没有额外平台资源时数组仍非空。macOS runner 的旧版 Bash 在 `set -u` 下展开空数组会报 `unbound variable`；不因此关闭 nounset 或额外安装 shell。

Windows 复用 `setup-msvc-env` 和 `setup-rusty-v8`，构建 CLI、Code Mode host、executor、`codex-command-runner.exe` 和 `codex-windows-sandbox-setup.exe`，按上游规范布局放入包中。Windows x64 保留上游的 `LIBSQLITE3_FLAGS=SQLITE_DISABLE_INTRINSIC`。使用原生 Windows runner，而不是另建 Linux 交叉编译链路，是为了直接复用上游 MSVC、原生音频构建及验证方式。

六个平台都以 `STABLE_GIT_COMMIT` 标记源码提交。发布构建保留 `CARGO_PROFILE_RELEASE_DEBUG=0`，不生成另行分发的调试符号；当前没有原生崩溃符号服务需求，额外符号产物会增加链接、存储和发布成本。

### TUI voice runtime

voice 复用上游 Bazel 目标、固定版本原生依赖、runtime receipt、许可文件及组装脚本。

- **Linux**：CLI 仍是 musl，voice helper 和音频库则采用同架构 GNU ABI，这是上游规定的组合。语音需要支持该 GNU ABI 的 Linux 环境；不能因为 CLI 是 musl，就推断整个语音包能在纯 musl 环境运行。
- **macOS**：复用上游原生 Bazel 构建和 `--macos_minimum_os=14.0`，voice 要求 macOS 14.0 或更新版本。上游 `macos_runtime.py` 已在改写动态库加载路径后给 dylib 做 ad-hoc 签名及验证；stage 保留这些字节，fork 只给 voice helper 增加同样的签名。签名完成后才 seal 并生成最终 manifest，不能在 hash 固定后修改二进制。
- **Windows**：`build_voice_windows.ps1` 下载 `openai/codex` 发布的固定 Cygwin 输入和源码快照，校验后调用上游准备脚本，再原生构建 MSVC helper 和 DLL。固定下载仓库很重要：上游 workflow 中的当前仓库表达式不能原样用于 fork。脚本不依赖私有远端构建服务。
- **Windows 临时目录**：保留上游 CI 的短路径做法，将 voice 的 `TMP` / `TEMP` 设为 `C:/b/t`，显式传入 Bazel action。公开 runner 默认的 `RUNNER~1` 路径会使 libffi wrapper 路径被加引号，而 Autoconf 展开 `CC` 时无法重新解释这些引号，导致编译器探测失败。
- **统一 stage/assemble**：`voice_package.py` 调用上游 `release_runtime.py` 和 `assemble_package.py`。Windows 额外通过上游 `windows_crt.py` 加入并校验固定的 Microsoft CRT。保留 runtime 文件清单、hash、来源提交和许可信息；不绕过缺失资源或来源不一致的错误。
- **同一提交**：CLI、voice helper、runtime 必须来自同一 fork 提交。客户端握手会核对 helper 的构建提交，assembler 也核对 runtime 来源。直接下载官方 helper 拼到 fork 中会违反这个约束，所以这里只复用公开的构建输入，不复用官方 helper 二进制。

规范包内部继续使用上游 Cargo 版本，例如 `0.163.0`；npm 版本仍是 `0.163.0-codeks.N`。这既保留 fork 发布标识，也满足上游语音 assembler 的版本契约，不需要修改上游版本解析规则。

### 缓存、打包与发布

workflow 统一设置 `PYTHONUTF8=1`，让各平台的 Python 脚本和子进程使用一致的 UTF-8 默认编码。Windows runner 的非 UTF-8 默认编码会使中文诊断或成功提示本身抛出编码异常，即使实际构建和验证已经完成。

Cargo 缓存 registry index、下载包和 git database；不缓存完整 target 目录。Bazel 缓存 repository 下载和本地 action 产物，disk cache 配置 2 GB 的清理目标。目标是复用依赖及原生音频构建成果，同时避免维护很大的 Rust release 输出缓存或引入远端构建凭据。构建仍可在没有缓存时完成。

`npm_package.py archive/collect` 复用上游校验完整包的布局、版本、voice 构建身份及文件 hash。完整包包含独立 `codex-exec-server` 和 metadata 的 `execServer`；上游 npm 打包器随后省略这两项，npm 用户仍可运行 `codex exec-server`。因此 `release_smoke.py` 为 npm 单独检查对应布局，复用上游目标、入口名和可执行权限判断，其余版本、voice 身份及 hash 校验共用；不为复用完整包校验器而给 npm 增加重复二进制。Windows 使用 ZIP，Linux/macOS 使用 tar.gz，用户解压后必须保留整个目录。`pack` 继续调用上游 npm 打包器，修改包名、仓库和来源信息，仅为实际构建的平台设置依赖：

- 主包名为 `@wacry/codeks`，仓库为 WAcry/codeks。
- 六个平台包仍采用上游的同包名、不同版本及 npm alias 机制。
- `codeks-npm.json` 记录版本、文件及 dist-tag。上游别名或包结构发生变化时，打包失败并要求适配，不生成猜测的映射。
- CI 上传 npm 和完整下载包两个 artifact 集合，保留 7 天。CLI、voice 中间产物先 tar 归档再跨 job 传输，保留 Unix 可执行权限。

发布先上传六个平台 npm 包，再上传主包并更新 `latest`。使用 npm trusted publishing：job 申请 `id-token: write`，不保存 npm token，公开仓库发布附带 npm provenance。已存在的版本跳过，允许失败后重跑。随后使用 `contents: write` 创建或复用当前 tag 的 GitHub Release，上传完整归档和校验文件；这些权限只给 publish job。

当前发布范围是 Linux、macOS、Windows x64 / ARM64 的 CLI 完整包和 npm 包。独立 app-server、responses-api-proxy、SDK、Python wheel 和平台安装器不另行发布，独立调试符号也不分发。上游的并行 Bazel CLI 发布链路不重复搭建，Bazel 只用于所需的 voice 构建。

macOS ad-hoc 签名只满足 Apple silicon 的代码完整性要求，不证明开发者身份，也不等于 Developer ID 签名和 Apple notarization。[Apple 明确说明](https://developer.apple.com/documentation/macos-release-notes/macos-big-sur-11_0_1-universal-apps-release-notes)，ad-hoc 签名可跨机器验证，但不能凭此通过 Gatekeeper。浏览器下载的归档可能携带 quarantine 并被 Gatekeeper 拦截；当前发行不承诺这种安装路径无提示。npm 使用 tar 解包程序文件，原生 smoke 验证包内程序和语音加载；不替用户移除 quarantine 或修改系统安全设置。若需要无提示的浏览器下载体验，再配置 Developer ID 和 notarization。Windows 同样暂不接入发行者签名凭据。

## 必要性

少了这个 workflow，fork 无法按要求在远端构建并发布这六个平台，使用方也无法获得可运行的完整 voice 资源。只增加 Windows CLI 或下载官方 voice helper 都不足以满足要求：前者没有音频运行时，后者与 fork 的构建身份不匹配。

上游构建、校验和组装逻辑已经具备所需能力，因此不新增业务补丁，也不修改上游 workflow；差异由 fork helper 和独立编排解决。workflow 必须放在 `.github/workflows/`，这是 0004 唯一的上游目录改动。

## 何时扩展

- 上游修改 Linux musl、macOS Darwin 或 Windows MSVC 的 release 步骤、Rust/Zig/V8 版本、构建前环境、随包资源或 bwrap 摘要机制：对照上游平台 workflow 同步本流程。
- 上游修改 voice Bazel 目标、GStreamer/CRT/Cygwin 固定输入、源代码归档、runtime receipt、helper 协议或同 commit 规则：先更新 fork 构建/组装/验证适配，确认许可信息与完整原生依赖仍随包发布。
- `build_codex_package.py`、`assemble_package.py` 的参数、版本约束或规范目录改变：调整 `voice_package.py`、归档步骤和 `release_smoke.py`，不通过改写来源元数据掩盖不匹配。
- `build_npm_package.py` 的包名、平台表、版本格式、alias 或启动器查找方式改变：更新 `npm_package.py` 并验证实际解包布局。需要增加平台时，同时更新构建矩阵、完整归档集合和原生 smoke。
- 新平台出现链接失败、内存不足或磁盘耗尽：基于对应 job 日志调整并发、swap 或缓存范围，不仅延长超时。缓存过大或命中率过低时，先衡量下载和恢复成本再改变策略。
- 有明确独立程序包、原生崩溃诊断或受签名策略限制的部署需求时，再讨论对应产物、符号或签名，不将其作为当前发布的前置条件。
- npm/GitHub 改变 trusted publishing、Action runner 或工具版本要求时，同步最小必要配置。

## 何时移除

如果不再需要 fork 自己的二进制和 npm 分发，可以移除本 workflow 及不再使用的 fork 打包适配。若上游提供不依赖其 runner、签名和发布身份的可参数化构建流程，优先复用，缩小我们的平台编排。某项语音构建或包适配变成上游稳定接口后，应删除对应重复代码，只保留必要调用；上游提供 npm 布局校验入口时，删掉 fork 中对应的布局检查并直接调用。

## 冲突处理

这是 fork 新增的 workflow，通常不会产生文本冲突。如果上游出现同名文件，给 fork workflow 另取 `codeks-*.yml` 名称，同时更新 npm trusted publisher 的 workflow 文件名及调用工具。

即使没有冲突，上游平台构建与 voice 脚本变化也可能破坏调用契约。同步时按 watch 路径复核当前版本的目标、输入、打包和运行检查，保留六个平台从同一提交构建、完整组装后再统一发布的顺序。helper 属于 0000，workflow 与本文属于 0004，分别 amend，保持一个补丁一个提交。

## 验证

- 本地只运行 `codeks.py check` 和必要的静态检查，不编译 Rust 或音频库。
- 使用 `codeks.py npm <分支>` 做不发布的远端试构建。六个 CLI job、六个 voice job、汇总打包及六个原生 smoke 都应成功，缺失任一平台或 voice 资源均不能进入 publish。
- 校验入口先将包根目录转为实际路径。Windows runner 的临时目录可能带 `RUNNER~1` 短名；上游逐文件检查会展开长路径，两边必须使用相同形式，避免将包内文件误判为越界。
- `smoke` 解包主包及当前平台 alias，运行 npm 启动器的 `codex --version` 和 rg，并校验 npm 布局、版本、voice receipt 和 hash。真实 helper 必须报告同一 build commit，完成 `hello → ready`、`initializeRuntime → runtimeReady`、`close → closed`，正常退出。
- macOS smoke 还逐个验证解包后的 CLI、Code Mode host、rg、voice helper 和全部 dylib 签名。通过该检查与运行检查仍不能替代 Gatekeeper 或真实设备验证。
- 这些检查验证的是随包资源、原生依赖加载和生命周期，**不等于麦克风/扬声器设备测试，也不等于真实语音后端连接或完整 TUI 语音对话实测**。实际使用语音时，还需要对应设备、系统权限及服务端能力。
- 检查 npm artifact 含一个主包、六个平台包和 `codeks-npm.json`；下载 artifact 含六份完整归档及各自 SHA256。
- 发布后确认 npm `latest` 和六个平台 dist-tag，GitHub Release 归档及校验文件齐全。目标系统中安装 `@wacry/codeks@<版本>` 或解压完整包，确认启动器和资源路径正确；有语音设备的机器再检查实际采集、播放和会话。

## 记录

- 2026-10-15 @ rust-v0.162.1：新增。
- 2026-10-15 @ rust-v0.162.1：第一次构建在编译 `codex-tui` 时因内存不足被杀，加上 swap，关闭 release 的 debuginfo，失败时打印内存和内核日志。
- 2026-10-15 @ rust-v0.162.1：仓库改为公开；发布改用 trusted publishing。第一次发布（0.162.1-codeks.4）用的是临时 token，因为 trusted publisher 只能配置给已经存在的包。
- 2026-10-17 @ rust-v0.163.0：同步上游包布局（#52213），随包资源为 bwrap 和 rg；沿用 Rust 1.95.0、Zig 0.14.0 和 Cargo 构建，确认上游新增的符号处理保留预编译输入。
- 2026-10-19 @ rust-v0.163.0：按用户需求扩展为 Linux/Windows x64 与 ARM64，加入同提交构建的 TUI voice runtime、完整 GitHub Release 下载包、原生打包 smoke 和有限构建缓存；CLI/voice 分 job，统一组装与发布，暂不扩展 macOS、二进制签名和调试符号。
- 2026-10-20 @ rust-v0.163.0：扩展 macOS x64/ARM64，复用标准免费 runner 和上游 voice 原生构建；使用无需证书的 ad-hoc 签名，并验证解包后的签名及 voice 初始化。未引入 Developer ID/notarization，明确区分运行完整性与 Gatekeeper 信任。
- 2026-10-20 @ rust-v0.163.0：首个 macOS ARM64 Rust release 编译成功后，打包因旧版 Bash 的空数组 nounset 行为失败；将共有打包参数放入数组，修复两种 macOS 架构的同一路径。

- 2026-10-23 @ rust-v0.164.0：CLI 与 voice job 的 Rust 工具链同步至 1.99.1，使用上游固定的安装 Action SHA；沿用六个平台的同提交构建、完整资源组装与原生 smoke。
- 2026-10-30 @ rust-v0.166.0：跟随上游改用 Rust 1.95.0；完整包增加同提交构建、strip 和 macOS ad-hoc 签名的 executor。npm 沿用上游裁剪，分开完整归档与 npm 布局校验，继续共用 voice 身份及 hash 校验。

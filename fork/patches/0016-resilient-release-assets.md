# 0016 稳健下载固定的 GitHub 构建资源

```toml
watch_paths = [
  "MODULE.bazel",
  "MODULE.bazel.lock",
  ".github/scripts/install-musl-build-tools.sh",
  ".github/scripts/run-bazel-ci.sh",
  "fork/tools/build_voice_windows.ps1",
]
watch_regex = [
  "github.com/.*/releases/download/",
  "(rules_cc|install-dotslash|--distdir)",
]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

2026-11-09，Codeks `codeks/rust-v0.167.0/3` 的六平台构建（GitHub Actions #42583373385）多路失败：

- macOS ARM64 和 Windows x64 Voice 的 DotSlash `github.com/facebook/dotslash/releases/download` 触发 HTTP 504；Windows x64 的后续构建工具无法准备完成。
- Linux ARM64 CLI 安装 musl OpenSSL 3.6.4 时，`github.com/openssl/openssl/releases/download` 返回 HTTP 504。
- macOS x64 及 Linux x64/ARM64 Voice 的 Bazel Bzlmod 下载 `github.com/bazelbuild/rules_cc/releases/download/0.2.20/...` 返回 HTTP 504，导致仓库获取失败。

这些错误发生在必需构建输入下载阶段；并不说明 Rust/Exec Server 业务代码编译不通过。此前 0015 已为 `rusty_v8` 使用 GitHub API 下载相同官方资产作为兜底；本补丁将受信任资产下载扩展到其余入口，而不是放宽检测或隐藏 CI。

## 改动

- **`.github/scripts/codeks_release_asset.py`**：新增统一的标准库 Python 下载工具，按指定仓库、tag、asset 名称从 GitHub Releases REST API 查询精确 asset ID，以 `Accept: application/octet-stream` 下载官方原始文件，采用临时文件和原子重命名；验证资产大小、官方 asset 元数据中的 SHA256（若存在）以及本地已固定的 SHA256。重复使用已有文件前也先验 SHA256；有限次重试 HTTP 408/429/5xx 和连接错误，非临时错误直接失败。可选令牌仅用于 API 元数据请求，不把凭据带到 Release 资产重定向域名。没有第三方镜像、备用版本或跳过摘要。
- **DotSlash**：该工具对 Linux、macOS、Windows 的 x64/ARM64 六种 runner 固定 `v0.5.9` 的原生资产和六平台对应 SHA256（macOS 共用通用可执行程序），检查 tar 只包含所需二进制并执行 `--version`，通过 `GITHUB_PATH` 供下游步骤使用；在 `.github/actions/setup-ci/action.yml` 和 `.github/workflows/codeks-npm.yml` 中替代不稳定的 `facebook/install-dotslash` 下载链路。Linux/macOS CI 和 Windows ARM64 均由同一路径安装，不改程序功能。
- **musl OpenSSL**：`.github/scripts/install-musl-openssl.sh` 中下载 `openssl-3.6.4.tar.gz` 改为上述工具，复用脚本原有固定 SHA256 `9bffaa...`；原先的 `sha256sum --check` 继续执行，编译参数及工具链保持不变。
- **Bazel Bzlmod**：`prefetch-bazel` 读取 `MODULE.bazel.lock` 中的 BCR `source.json` 条目，每项从 Bazel Central Registry 获取元数据并先验证其 *原始字节* SHA256 等于 lockfile 固定值，仅挑选引用 GitHub Releases `releases/download` 的 URL，从 `integrity`/`sha256` 提取源码已锁定的归档 digest。经 GitHub 官方资产 API 下载、逐项校验后放入 Bazel `--distdir`；发现同文件名不同 SHA 或来源版本变动会停止。0.167.0 的 lockfile 共核对 45 个 BCR source.json，预取 36 个 GitHub release 资产（总计约 103 MB），其中含阻断构建的 `rules_cc 0.2.20`。相同归档无需依赖失败的浏览器下载 URL。
- **流程接入**：`.github/workflows/codeks-npm.yml` Voice 六平台 job 预取 Bazel distdir；Unix Bazel 命令加 `--distdir`。Windows 构建通过 0000 下的 `fork/tools/build_voice_windows.ps1` 小调整读取 `CODEKS_BAZEL_DISTDIR` 并传给同一个 Bazel 命令，手动运行时保留缺省的空 distdir。两个 workflow 的 prepare/check 步骤均运行新增的 `.github/scripts/test_codeks_release_asset.py` 离线单元测试。

`.github/workflows/codeks-ci.yml` 的 check job 和 codeks-npm 的 prepare job 都执行新增的离线下载器单元测试，避免构建开始后才发现格式或验证逻辑不兼容。

## 必要性

新版本需在六平台构建来自 *Codeks fork* 的 CLI、独立 Exec Server、voice 和 sandbox 资源。`github.com/releases/download` 反复 504 并不意味着 GitHub 官方 Releases API 的原资产不可用；只重试会重复失效。直接屏蔽失败、复用官方二进制、跳过校验或改用不明镜像，会破坏构建身份及供应链完整性。通过公开官方 API 与已有 Bazel/Cargo 摘要固定输入最少改变行为，兼顾跨平台与未来上游同步成本。

## 何时扩展

- Bazel lockfile 采用新的 registry 目录、`source.json` 格式、URL 或 integrity 算法时，按 Bazel 实际下载约定更新预取逻辑，不忽略 SHA 不符。
- GitHub Releases API 的资产、签名、tag 或重定向行为变化时，确认确实下载到同一个固定资产，API 不能用时失败而不是绕过。
- 如果后续失败来自并非 BCR GitHub Releases 的其他网络来源，根据日志在对应边界增加有限且受信任的下载途径，不将所有外部 URL 一概改写。

## 何时移除

上游引入稳定且经过完整性校验的等价下载回退，或者 GitHub 官方 API 不再适用时，可收回此工具及最小挂载点；不要在无其他校验来源时移除锁定摘要。

## 冲突处理

优先沿用上游新的 Rust、Bazel、DotSlash、OpenSSL 版本及各平台构建流程。`MODULE.bazel.lock` 是准确的模块清单；不要把 `rules_cc 0.2.20`、DotSlash 0.5.9 或 OpenSSL 3.6.4 硬编码为将来版本的替代。新的 SHA 需要上游锁文件或官方发布资产支持。Windows build helper 属于 0000，upstream action/workflow/source 脚本及本文属于 0016，分别按补丁规范维护。

## 验证

- `python3 .github/scripts/test_codeks_release_asset.py` 在无网络环境测试六平台选择、SHA、危险路径阻止、BCR 锁校验、归档目录和冲突处理。
- 在 Windows 本机实际执行 DotSlash v0.5.9 `--version`、OpenSSL 3.6.4 GitHub API 下载并核验 SHA256、BCR 45 项锁文件校验和 36 个官方资产完整下载及 SHA256；通过 `python -m py_compile`、Bash `-n`、YAML 解析和 `codeks.py check`。
- 在 GitHub `codeks-ci` 和 `codeks-npm` 重新运行验证 Linux/macOS/Windows 的真实构建、Rust 测试及最后的发布归档；没有增加任何 `ci_skip` 条目。

## 记录

- 2026-11-09 @ rust-v0.167.0：新增，用 GitHub 官方 API 获取全部锁定的构建资源，解决 DotSlash、musl OpenSSL 与 Bazel 依赖下载 HTTP 504，保持 SHA 校验。

# 0015 GitHub 构建依赖的可靠下载

```toml
watch_paths = [
  ".github/scripts/rusty_v8_bazel.py",
  "third_party/v8/**",
  ".github/workflows/codeks-ci.yml",
  ".github/workflows/codeks-npm.yml",
  "MODULE.bazel",
  "MODULE.bazel.lock",
  ".github/scripts/install-musl-build-tools.sh",
  ".github/scripts/run-bazel-ci.sh",
  "fork/tools/build_voice_windows.ps1",
]
watch_regex = [
  "rusty.v8|setup-rusty-v8|rusty-v8-v",
  "github.com/.*/releases/download/",
  "(rules_cc|install-dotslash|--distdir)",
]
backport_of = [
]
upstream = [
]
ci_skip = [
]
```

## 背景

2026-11-08 在 Codeks 0.167.0-codeks.2 上执行 Rust CI 与六平台发布构建时，多个 GitHub Actions runner 在编译前调用 `.github/actions/setup-rusty-v8/action.yml`，通过 `https://github.com/openai/codex/releases/download/rusty-v8-v152.2.0/` 下载预编译 V8 产物时连续收到 HTTP 504。Linux/macOS/Windows 出现相同现象，和本 fork 的 Rust 改动、Exec Server 代码或构建选项无关。

验证相同 tag 在公开的 GitHub Releases REST API 可查询到确切的三个 release assets，`GET /repos/openai/codex/releases/assets/<id>` 携带 `Accept: application/octet-stream` 则可以重定向到可下载的原始资产。构建不应该因为下载展示 URL 的临时故障而中止。

2026-11-09，Codeks `codeks/rust-v0.167.0/3` 的六平台构建（GitHub Actions #42583373385）多路失败：

- macOS ARM64 和 Windows x64 Voice 的 DotSlash `github.com/facebook/dotslash/releases/download` 触发 HTTP 504；Windows x64 的后续构建工具无法准备完成。
- Linux ARM64 CLI 安装 musl OpenSSL 3.6.4 时，`github.com/openssl/openssl/releases/download` 返回 HTTP 504。
- macOS x64 及 Linux x64/ARM64 Voice 的 Bazel Bzlmod 下载 `github.com/bazelbuild/rules_cc/releases/download/0.2.20/...` 返回 HTTP 504，导致仓库获取失败。

这些错误发生在必需构建输入下载阶段；并不说明 Rust/Exec Server 业务代码编译不通过。同一补丁先为 `rusty_v8` 使用 GitHub API 下载相同官方资产作为兜底；本补丁将受信任资产下载扩展到其余入口，而不是放宽检测或隐藏 CI。

## 改动

- `.github/actions/setup-rusty-v8/action.yml` 新增 `download_release_asset` shell helper：首先对既有浏览器下载 URL 做有限重试；发生错误时读取该官方 release tag 的公开 REST API 元数据，精确按资产名称找到 id，再通过 asset API 下载。第一次回退后，剩余两个资产直接复用同一份 release 元数据与 API 路径；避免对已失效的 URL 重复等待。下载失败或资产名不存在时仍正常报错，绝不静默使用旧版或其他架构的构建产物。
- 原有的两级完整性校验 **不变**：先核对官方 `.sha256` 清单的字节 SHA256 是否等于源码中固定在 `third_party/v8/` 的受信任摘要，再对预编译库和 Rust binding 源文件逐项做 SHA256 校验。两个下载入口得到的文件必须通过同一检查。
- 通过公开的 GitHub API 访问官方公开资产，不依赖 GitHub Actions token、私有下载镜像、第三方资源或额外软件包；仍使用上游已有的 `curl`、`bash`、`python3`。只处理资源获取方式，不修改 Rust 代码、沙箱能力、版本标识或发布内容。

- **`.github/scripts/codeks_release_asset.py`**：新增统一的标准库 Python 下载工具，按指定仓库、tag、asset 名称从 GitHub Releases REST API 查询精确 asset ID，以 `Accept: application/octet-stream` 下载官方原始文件，采用临时文件和原子重命名；验证资产大小、官方 asset 元数据中的 SHA256（若存在）以及本地已固定的 SHA256。重复使用已有文件前也先验 SHA256；有限次重试 HTTP 408/429/5xx 和连接错误，非临时错误直接失败。可选 Actions GitHub 令牌用于 API 元数据请求及资产请求，绕过每小时 60 次的未认证 API 速率限制；跨域 HTTP 重定向时显式剥离 Authorization，防止凭据流向 Release 资产 CDN。没有第三方镜像、备用版本或跳过摘要。
- **DotSlash**：该工具对 Linux、macOS、Windows 的 x64/ARM64 六种 runner 固定 `v0.5.9` 的原生资产和六平台对应 SHA256（macOS 共用通用可执行程序），检查 tar 只包含所需二进制并执行 `--version`，通过 `GITHUB_PATH` 供下游步骤使用；在 `.github/actions/setup-ci/action.yml` 和 `.github/workflows/codeks-npm.yml` 中替代不稳定的 `facebook/install-dotslash` 下载链路。Linux/macOS CI 和 Windows ARM64 均由同一路径安装，不改程序功能。
- **musl OpenSSL**：`.github/scripts/install-musl-openssl.sh` 中下载 `openssl-3.6.4.tar.gz` 改为上述工具，复用脚本原有固定 SHA256 `9bffaa...`；原先的 `sha256sum --check` 继续执行，编译参数及工具链保持不变。
- **Bazel Bzlmod**：`prefetch-bazel` 读取 `MODULE.bazel.lock` 中的 BCR `source.json` 条目，每项从 Bazel Central Registry 获取元数据并先验证其 *原始字节* SHA256 等于 lockfile 固定值，仅挑选引用 GitHub Releases `releases/download` 的 URL，从 `integrity`/`sha256` 提取源码已锁定的归档 digest。经 GitHub 官方资产 API 下载、逐项校验后放入 Bazel `--distdir`；发现同文件名不同 SHA 或来源版本变动会停止。0.167.0 的 lockfile 共核对 45 个 BCR source.json，预取 36 个 GitHub release 资产（总计约 103 MB），其中含阻断构建的 `rules_cc 0.2.20`。相同归档无需依赖失败的浏览器下载 URL。
- **流程接入**：`.github/workflows/codeks-npm.yml` Voice 六平台 job 预取 Bazel distdir；Unix Bazel 命令加 `--distdir`。Windows 构建通过 0000 下的 `fork/tools/build_voice_windows.ps1` 小调整读取 `CODEKS_BAZEL_DISTDIR` 并传给同一个 Bazel 命令，手动运行时保留缺省的空 distdir。两个 workflow 的 prepare/check 步骤均运行新增的 `.github/scripts/test_codeks_release_asset.py` 离线单元测试。

`.github/workflows/codeks-ci.yml` 的 check job 和 codeks-npm 的 prepare job 都执行新增的离线下载器单元测试，避免构建开始后才发现格式或验证逻辑不兼容。

## 必要性

V8 原生库是 Codeks CLI、Exec Server 和 CI 编译的必需输入。直接重新运行 workflow 只能寄希望于 GitHub 展示 URL 自行恢复；持续 504 会使所有平台同时失去发布能力。与增加镜像、关闭校验或将旧版构建产物硬编码到新 release 相比，官方同一资产的公开 API 回退修改范围更小，也保留了仓库原有可复现性和信任链。

新版本需在六平台构建来自 *Codeks fork* 的 CLI、独立 Exec Server、voice 和 sandbox 资源。`github.com/releases/download` 反复 504 并不意味着 GitHub 官方 Releases API 的原资产不可用；只重试会重复失效。直接屏蔽失败、复用官方二进制、跳过校验或改用不明镜像，会破坏构建身份及供应链完整性。通过公开官方 API 与已有 Bazel/Cargo 摘要固定输入最少改变行为，兼顾跨平台与未来上游同步成本。

## 何时扩展

- 上游更改 `.github/actions/setup-rusty-v8/action.yml` 的下载接口、release 命名、manifest 格式，或 `third_party/v8/` 中的 pinned V8 版本时，确认 REST API 资产名仍匹配、校验逻辑没有被绕过。
- 如果 GitHub 修改 release asset REST API 或停止对公开资产提供未经身份认证的重定向，优先更新到官方支持的访问机制，再考虑复用 Actions cache；不能跳过 SHA256。

- Bazel lockfile 采用新的 registry 目录、`source.json` 格式、URL 或 integrity 算法时，按 Bazel 实际下载约定更新预取逻辑，不忽略 SHA 不符。
- GitHub Releases API 的资产、签名、tag 或重定向行为变化时，确认确实下载到同一个固定资产，API 不能用时失败而不是绕过。
- 如果后续失败来自并非 BCR GitHub Releases 的其他网络来源，根据日志在对应边界增加有限且受信任的下载途径，不将所有外部 URL 一概改写。

## 何时移除

若上游加入可靠的等价多入口下载、合理重试或缓存机制，可删除本 helper 并采用上游实现；如果所有 Codeks workflow 不再依赖该 action，也可以移除。

上游引入稳定且经过完整性校验的等价下载回退，或者 GitHub 官方 API 不再适用时，可收回此工具及最小挂载点；不要在无其他校验来源时移除锁定摘要。

## 冲突处理

保留上游最新 V8 版本、目标三元组、预编译产物和两层校验作为事实来源，只把 API 回退放在实际下载点。遇到参数或校验形式变化，应优先适配上游的新下载路径，不能保留已经过时的发布 tag 或资产名。

优先沿用上游新的 Rust、Bazel、DotSlash、OpenSSL 版本及各平台构建流程。`MODULE.bazel.lock` 是准确的模块清单；不要把 `rules_cc 0.2.20`、DotSlash 0.5.9 或 OpenSSL 3.6.4 硬编码为将来版本的替代。新的 SHA 需要上游锁文件或官方发布资产支持。Windows build helper 属于 0000，upstream action/workflow/source 脚本及本文属于 0015，分别按补丁规范维护。

## 验证

- 对复合 action 的 shell 运行 `bash -n`，确认 Bash 解析通过；用公开 release API 的真实响应核对资产名称与 id 的解析。
- 原 URL 正常时继续使用原行为；遇到 504 时回退 API 下载，校验 `.sha256` 清单、预编译库和 binding；HTTP 错误、资产缺失或摘要错误均须停止构建。
- 在 `codeks-ci` 和 `codeks-npm` 的 Linux、macOS、Windows runner 上执行原有构建，不新增测试跳过项。

- `python3 .github/scripts/test_codeks_release_asset.py` 在无网络环境测试六平台选择、SHA、危险路径阻止、BCR 锁校验、归档目录和冲突处理。
- 在 Windows 本机实际执行 DotSlash v0.5.9 `--version`、OpenSSL 3.6.4 GitHub API 下载并核验 SHA256、BCR 45 项锁文件校验和 36 个官方资产完整下载及 SHA256；通过 `python -m py_compile`、Bash `-n`、YAML 解析和 `codeks.py check`。
- 在 GitHub `codeks-ci` 和 `codeks-npm` 重新运行验证 Linux/macOS/Windows 的真实构建、Rust 测试及最后的发布归档；没有增加任何 `ci_skip` 条目。

## 记录

- 2026-11-08 @ rust-v0.167.0：新增。处理上游 GitHub Releases 浏览器下载 URL 返回 504、同一资产 API 可以获取的构建故障。
- 2026-11-09 @ rust-v0.167.0：新增，用 GitHub 官方 API 获取全部锁定的构建资源，解决 DotSlash、musl OpenSSL 与 Bazel 依赖下载 HTTP 504，保持 SHA 校验。
- 2026-11-09 @ rust-v0.167.0：实际 /4 发布 job 发现未认证 asset API 触发 HTTP 403 rate limit；在资产 API 请求使用 Actions token，并在跨域重定向时删除 Authorization，增加防泄漏单元测试。

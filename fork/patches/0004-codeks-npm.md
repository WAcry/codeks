# 0004 fork npm 包：codeks-npm workflow

```toml
watch_paths = [
  ".github/workflows/rust-release.yml",
  ".github/scripts/install-musl-build-tools.sh",
  ".github/scripts/install-musl-openssl.sh",
  ".github/scripts/verify-zsh-manifest.sh",
  ".github/actions/setup-rusty-v8/**",
  "codex-cli/**",
  "scripts/build_codex_package.py",
  "scripts/codex_package/**",
  "codex-rs/rust-toolchain.toml",
]
watch_regex = []
backport_of = []
upstream = []
```

## 背景

其他个人项目（例如 ~/git/rodex）通过 npm 依赖 `@openai/codex` 来使用 Codex。要让它们用上 fork 的二进制，就要把 fork 打成结构相同的 npm 包。

上游的 npm 包由 `rust-release.yml` 在推送 `rust-v*` tag 时构建并发布，流程是：

1. 在各平台构建原生二进制；
2. 用 `scripts/build_codex_package.py` 组装成 codex package 目录（`bin/`、`codex-resources/`、`codex-path/`）；
3. 用 `codex-cli/scripts/build_npm_package.py` 生成一个主包和若干平台包。平台包是同一个包名的不同版本，例如 `@openai/codex@0.162.1-linux-x64`。主包只有 `bin/codex.js`，通过 `optionalDependencies` 别名（`"@openai/codex-linux-x64": "npm:@openai/codex@0.162.1-linux-x64"`）引用平台包。

`rust-release.yml` 只能由上游 tag 触发，还依赖 OpenAI 的 runner、签名和凭据，在 codeks 里不能用，已经停用。

## 改动

新增 `.github/workflows/codeks-npm.yml`，只能手动触发，用 `codeks.py npm` 启动。打包逻辑在 `fork/tools/npm_package.py`（属于 0000），workflow 只负责编排上游现成的脚本。

build job（ubuntu-24.04）：

开始前先加 12 GB swap（放在 `/mnt` 上，空间不够时放在 `/` 上），并打印 CPU、内存和磁盘。第一次构建时，进程在编译 `codex-tui` 时被杀掉（退出码 143），日志里没有编译错误，最可能是标准 runner 的内存不够。

1. 运行 `codeks.py check`，再用 `npm_package.py version` 决定版本号。要求发布、但 ref 不是 codeks tag 时，在这里就失败，不浪费构建时间。
2. 构建步骤照搬上游 release 里 x86_64-unknown-linux-musl 的 job：
   - 安装 apt 依赖；
   - Rust 1.95.0 加上 musl target，使用隔离的 `CARGO_HOME`；
   - 安装 Zig 0.14.0，运行 `install-musl-build-tools.sh`；
   - 关闭 aws-lc 的 jitter entropy；
   - 用 `setup-rusty-v8` 准备预编译的 V8；
   - 先构建并 strip `bwrap`，把它的 sha256 作为 `CODEX_BWRAP_SHA256` 编进 codex，codex 运行时会校验随包的 bwrap；
   - 用 release profile 构建 `codex` 和 `codex-code-mode-host`，然后 strip。和上游不同，这里设置 `CARGO_PROFILE_RELEASE_DEBUG=0`：上游保留 line tables 是为了单独归档符号，我们最后会 strip 掉，所以不生成，省内存和时间，产物的行为不变。
   - 构建失败时打印内存和 `dmesg`，用来区分内存不足和编译错误。
3. zsh 用上游 release 固定的那一版：从 `rust-release.yml` 读出 `CODEX_ZSH_RELEASE_TAG` 和 `CODEX_ZSH_MANIFEST_SHA256`，从 openai/codex 的 release 下载 manifest 并校验。
4. 用 `scripts/build_codex_package.py` 组装 codex package：二进制、bwrap、zsh，以及按 DotSlash manifest 下载的 rg。
5. `npm_package.py pack` 调用上游的 `build_npm_package.py` 生成主包和 linux-x64 平台包，然后：
   - 包名改成 `@wacry/codeks`，`repository` 改成 WAcry/codeks；
   - 主包的 `optionalDependencies` 只保留实际构建的平台，指向 `npm:@wacry/codeks@<版本>-linux-x64`；
   - 主包写入 `codeks` 来源字段；
   - 运行 `npm pack`，并写一份 `codeks-npm.json`，记录每个 tarball 的包名、版本和 dist-tag。

   上游的包名、平台列表或别名写法变了时，`npm_package.py` 会报错停下，不会生成错误的包。
6. `npm_package.py smoke` 把 tarball 解包到临时的 `node_modules`：主包放在包名下，平台包放在别名下，和 npm 安装后的布局一样。然后通过上游的 `bin/codex.js` 运行 `codex --version`，输出必须是 `codex-cli <上游版本>`。
7. 上传 artifact `codeks-npm-<版本>`，内容是两个 tgz 和 `codeks-npm.json`，保留 30 天。

publish job 只在输入 `publish=true` 时运行：

- 下载 artifact，用 Node 24 运行 `npm_package.py publish`；
- 先发布平台包（dist-tag `linux-x64`），最后发布主包（dist-tag `latest`），这样主包成为 latest 时，它引用的平台包已经存在；
- 已经发布过的版本会跳过，所以失败后可以直接重跑；
- 凭据：配置了 `NPM_TOKEN` secret 就用 token；没有配置时，npm 会尝试 trusted publishing（OIDC），所以 job 申请了 `id-token: write` 权限。

和上游 release 相比，有意省掉了这些：

- 只构建 linux-x64，没有 linux-arm64、macOS 和 Windows；
- 不含 voice runtime（上游用 Bazel 单独构建），所以 TUI 的本地语音模式不可用；app-server 不依赖它；
- 不签名（cosign、macOS 签名），不保留符号文件；
- 不生成 GitHub Release、DMG、Python wheel，也不发布 `codex-responses-api-proxy` 和 `codex-sdk`；
- 构建不用缓存，因为 npm 包很少构建。

## 必要性

少了它，fork 的二进制只能在本地构建，违背“本地默认不编译”；其他项目也没办法通过 npm 用上 fork。上游的 release workflow 只能由 `rust-v*` tag 触发，并且依赖 OpenAI 的 runner 和凭据，不能复用。workflow 文件只能放在 `.github/workflows/`，不能放进 `fork/`。

## 何时扩展

- 上游改了 `rust-release.yml` 里 musl 的构建步骤，例如 Zig 版本、环境变量、新增的构建前步骤、新的随包二进制、bwrap 的处理方式：对照着同步过来。
- `build_codex_package.py` 的参数或 codex package 的布局变了：跟着改 “Assemble the codex package” 这一步。
- `build_npm_package.py` 里的 `CODEX_NPM_NAME`、`CODEX_PLATFORM_PACKAGES`、平台包版本的格式，或者 `optionalDependencies` 的写法变了：`npm_package.py` 会报错，按新结构调整它（`amend 0000`）。
- `codex-cli/bin/codex.js` 改了找平台包的方式（现在按 `@openai/codex-<平台>` 别名查找）：`smoke` 会失败，按新方式调整。
- `rust-release.yml` 不再写 zsh 的 tag 和 sha256：上游不再随包带 zsh 时，删掉下载 zsh 的那一步和 `--zsh-manifest`；改成别的来源时，跟着改。rust-v0.163.0 就是前一种情况（#52213）。
- 构建又在没有编译错误的情况下被杀掉（退出码 137 或 143）：看失败时打印的内存和 `dmesg`，加大 swap，或者减少并行（`CARGO_BUILD_JOBS`）。
- 需要更多平台：仿照上游的 matrix 增加 job，平台包按 `CODEX_PLATFORM_PACKAGES` 自动生成。私有仓库里 macOS runner 按 10 倍计费、Windows 按 2 倍。

## 何时移除

不再需要 fork 的 npm 包：没有项目通过 npm 使用 fork，或者使用方都换成了别的分发方式。

## 冲突处理

新文件，正常不会冲突。如果上游新增了同名文件，就把我们的文件改名为另一个 `codeks-*.yml`。

## 验证

- `codeks.py npm <分支>` 试构建：build job 通过，`smoke` 打印 `codex-cli <版本>`，artifact 里有两个 tgz 和 `codeks-npm.json`。
- 发布后：`npm view @wacry/codeks dist-tags` 里 `latest` 是新版本；在空目录 `npm i @wacry/codeks@<版本>`，再运行 `npx codex --version`。

## 记录

- 2026-10-15 @ rust-v0.162.1：新增。
- 2026-10-15 @ rust-v0.162.1：第一次构建在编译 `codex-tui` 时因内存不足被杀，加上 swap，关闭 release 的 debuginfo，失败时打印内存和内核日志。

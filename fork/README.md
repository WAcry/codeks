# codeks

codeks 是 [openai/codex](https://github.com/openai/codex) 的自用 fork，仓库为 [WAcry/codeks](https://github.com/WAcry/codeks)（公开）。目标是用尽量少的补丁，高频跟随上游发布。

fork 自己的东西都在 `fork/` 目录，文档用中文。上游文件只在补丁里改，每个补丁都有一份文档，写清为什么改、改了什么、上游怎样变化时要扩展或移除它。

## 原则

改动要小、要精炼。能新建文件就不要改热点；能配置或适配层解决就不要分叉业务逻辑。每一处补丁都要能说清：少了它哪条原则或功能会坏。补丁越少，以后和上游合并时冲突越少。

具体怎样落实见 [PATCHING.md](PATCHING.md)。

## 仓库模型

```text
rust-v0.162.1 ── 0000 ── 0001 ── 0002 ── 0003 ── 0004   ← main
（上游基线）     （补丁栈：一个补丁一个提交）
```

- **基线**：`main` 建在上游的一个 release tag 上，默认用最新的 stable（`rust-vX.Y.Z`）。
- **补丁栈**：基线之上是线性的补丁提交。一个补丁 = 一个提交 = 一份文档 `fork/patches/NNNN-<slug>.md`。提交带 trailer `Codeks-Patch: NNNN`。工具从 `HEAD` 往下找，第一个不带这个 trailer 的提交就是基线，所以基线不需要另外记录。
- **0000**：最底层的特殊补丁，只包含 `fork/` 目录（文档和工具），不会和上游冲突。
- **同步**：把补丁栈 rebase 到新的上游 tag 上，见 [SYNC.md](SYNC.md)。每次同步都会改写 `main`，推送使用 `--force-with-lease`。
- **版本 tag**：每次发布都打一个 annotated tag `codeks/<基线>/<n>`，例如 `codeks/rust-v0.162.1/1`。tag 推送后不再移动，里面保存了这个版本的完整补丁栈、与上一版本的差异和同步报告。

基线选 release tag，不选上游 `main`，原因有三个：

1. 上游 release tag 不在 `main` 上，而在发布分支上，带有 cherry-pick 和修改版本号的提交。跟 `main` 拿不到和官方发布一致的代码。
2. tag 上 `codex-rs/Cargo.toml` 是真实版本号，`main` 上是 `0.0.0`。客户端会把 `major.minor.patch` 作为 `client_version` 发给模型目录接口，服务端按 `minimal_client_version` 过滤模型，`0.0.0` 可能看不到新模型。
3. 上游 stable 发布本来就很频繁（例如 0.161.0、0.162.0、0.162.1 分别在 10-06、10-08、10-09 发布），跟 tag 已经足够高频。

急需上游还没发布的修复时，优先做回移补丁（[PATCHING.md](PATCHING.md)「回移补丁」），其次临时用 alpha tag 做基线。

## 版本号

一个 fork 版本有三个编号，一一对应：

| 用在哪里 | 例子 | 规则 |
|---|---|---|
| git tag | `codeks/rust-v0.162.1/3` | 基线 rust-v0.162.1 上的第 3 次发布。基线不变、补丁变了，n 加一；换了基线，n 从 1 重新开始 |
| npm 包 | `@wacry/codeks@0.162.1-codeks.3` | 由 tag 推出：`<上游版本>-codeks.<n>`。分支试构建是 `0.162.1-codeks.dev.g<sha>`，不能发布 |
| 二进制 | `codex-cli 0.162.1` | 保持上游版本，不加后缀 |

二进制不加后缀。完整版本号会出现在 User-Agent 和 `version` 请求头里，写进会话记录的 `cli_version`，也用于本地的升级检查。这些地方都按上游版本工作；加上后缀没有好处，而且 semver 认为 `0.162.1-codeks.3` 比 `0.162.1` 旧，升级检查会因此提示升级到 0.162.1。

npm 版本号是预发布的形式。这只影响 npm 里的排序和范围匹配：例如 `^0.162.1-codeks.3` 匹配不到 `0.163.0-codeks.1`。所以使用方要写死版本号，或者用 dist-tag `latest`。

- 查看一个版本：`git show codeks/rust-v0.162.1/1`。
- 对比两个版本的补丁：`git range-diff <旧基线>..<旧 tag> <新基线>..<新 tag>`。

## 目录

| 路径 | 内容 |
|---|---|
| `fork/README.md` | 本文：模型、版本、命令、审查、验证、构建、npm、GitHub 设置 |
| `fork/PATCHING.md` | 补丁规范：要不要改、怎样改最小、文档和提交怎么写 |
| `fork/SYNC.md` | 同步上游：流程、冲突处理、同步后审查、回滚 |
| `fork/patches/` | 每个补丁一份文档，`TEMPLATE.md` 是模板 |
| `fork/tools/codeks.py` | 辅助脚本，只依赖 Python 3.11+ 标准库、git 和 gh |
| `fork/tools/npm_package.py` | 生成、检查和发布 npm 包，由 `codeks-npm` workflow 调用 |

现有补丁可以用 `codeks.py status` 查看，或者直接看 `fork/patches/`。

## 初始化

```bash
git clone https://github.com/WAcry/codeks.git ~/git/codeks
cd ~/git/codeks
python3 fork/tools/codeks.py setup
```

`setup` 可以重复运行。它会：

- 添加 `upstream`（openai/codex），并且只拉取上游的 `main` 和 `rust-v*` tag；
- 让 `origin` 拉取 `codeks/*` tag；
- 开启 `rerere`，记住冲突的解决方式，下次自动套用；
- 设置 `pull.ff=only`。

## 命令

在仓库或任意一个 worktree 里运行 `python3 fork/tools/codeks.py <命令>`：

| 命令 | 作用 |
|---|---|
| `status [--fetch]` | 基线、补丁列表、上游有没有新 stable、和 origin 的关系 |
| `check [REV]` | 检查补丁栈和文档是否合规，并用本机的私有名称清单扫描，几秒钟，不编译 |
| `new <slug> <标题>` | 分配下一个补丁编号，从模板生成文档 |
| `amend <ID>` | 把已暂存的改动并入补丁 ID 的提交 |
| `report [TARGET]` | 只读：列出从当前基线到 TARGET 之间与各补丁相关的上游提交 |
| `sync [TARGET]` | 把当前分支的补丁栈 rebase 到 TARGET（默认最新 stable） |
| `pr [--title 标题] [--ready] [--ci]` | 推送当前分支，创建或更新用来审查的 draft PR |
| `publish [-m 说明] [--no-ci] [--force]` | 检查后打 tag，推送 `main` 和 tag，处理 PR，触发 CI |
| `ci [分支] [--crates "a b"]` | 推送分支并在 GitHub 上运行 CI |
| `npm [tag 或分支] [--publish]` | 在 GitHub 上构建 npm 包，可选发布 |
| `pull [--force]` | 把本地 `main` 更新到 `origin/main` |

`main` 每次同步都会被改写，所以不要对它用 `git pull`，要用 `codeks.py pull`。

## 日常开发

在 worktree 里开发，不直接改 `~/git/codeks`：

```bash
cd ~/git/codeks && python3 fork/tools/codeks.py pull
git worktree add -b patch/<slug> ~/git/.worktrees/codeks/<slug> main
cd ~/git/.worktrees/codeks/<slug>
```

- 新补丁：`codeks.py new <slug> <标题>`，写代码和文档，然后按它打印的命令提交。
- 改已有补丁：改完 `git add`，再 `codeks.py amend <ID>`。改 `fork/` 的文档或工具就是 `amend 0000`。
- 删除补丁：`git rebase -i <基线>`，删掉对应的那一行，文档会随提交一起消失。基线可以用 `codeks.py status` 查看。
- 完成后：`codeks.py check`，需要审查时先 `codeks.py pr`，然后 `codeks.py publish`，最后删掉 worktree 和分支。

仓库是公开的，保密规则见根目录的 `AGENTS.md`。`check` 用本机的 `~/.config/codeks/private-terms` 扫描每个补丁的新增内容、提交说明、作者和提交者；`pr` 和 `publish` 推送前还会扫描分支名、PR 文字和 tag 说明。这个清单不在仓库里，没有清单时（例如在 CI 里）不扫描。PR 的 range-diff 会显示被删掉的旧内容，删除私有名称时它可能含有这些名称，这时 `pr` 不附 range-diff。

提交的作者和 tag 的创建者也会公开。codeks 用仓库级的 `user.name` 和 `user.email`（`git config` 不加 `--global`，所有 worktree 共用），新 clone 下来要重新设置。

`publish` 会改写 `main`，所以它要求当前分支见过 `origin/main` 现在的版本：分支是从它创建的，或者已经 rebase 到它上面。分支创建之后 `main` 又发布过时，`publish` 会拒绝，避免把那次发布的内容覆盖掉。这时先把分支更新到最新的 `main`。

### 用 PR 审查

想先看看改动再发布时，在开发分支上运行 `codeks.py pr`。它推送分支，创建一个 draft PR；再次运行会更新 PR 的描述。

- PR 只用来审查，不在 GitHub 上合并。描述里有相对 `main` 的补丁变化（range-diff），以及和 `main` 的净差异链接；同步 PR 还附上同步报告。
- 按审查意见修改：照常 `amend` 或新增补丁，再运行一次 `codeks.py pr`。
- 审查通过后，在这个分支的 worktree 里运行 `codeks.py publish`。它把 `main` 和这个分支推到同一个提交，GitHub 会自动把 PR 标为 Merged，然后删除远端分支。
- 在 GitHub 网页上提交的修改建议，会成为分支上的新提交，不带 trailer。`pr` 和 `publish` 发现远端有本地没有的提交时会停下：先把它们取回，并入对应的补丁。
- draft PR 在网页上不能合并，仓库也只允许 rebase 合并（`setup --github` 设置）。万一在网页上合并了，用 [SYNC.md](SYNC.md)「回滚」把 `main` 退回上一个 tag，再从分支 `publish`。

## 验证与 CI

本地默认不编译、不运行、不测试。补丁通常很小，一般不会破坏上游，而 codex 很大，完整编译和测试代价很高。本地只跑 `codeks.py check`，编译和测试交给 GitHub Actions 上的 `codeks-ci`（补丁 0003）。

- 触发方式：`publish` 推送后自动触发一次；开发分支需要时用 `codeks.py ci` 或 `codeks.py pr --ci`。普通的提交和推送都不会触发。
- GitHub 只能手动触发 `main` 上已有的 workflow。新增 workflow 的补丁要先 `publish`，之后才能触发它，包括在分支上触发。
- 检查内容：每次都检查补丁栈和文档。补丁改了 `codex-rs/` 时，再做格式检查、编译 `codex` 和测试辅助程序，并跑补丁涉及的 crate 的测试。
- 成本：公开仓库的标准 runner 不计费，但运行很慢。2026-10-15 实测（当时仓库还是私有，runner 是 2 核）：只检查补丁栈时约 1 分钟；有 Rust 补丁、没有缓存时约 24 分钟（清理磁盘 2 分钟，编译 16 分钟，测试一个小 crate 4 分钟），缓存约 1.7 GB。所以不要每个提交都跑。

## 构建

只有要用新版本时才在本地构建。需要别的项目使用 fork 时，优先用下面的 npm 包，在 GitHub 上构建。

本地构建要放在单独的 worktree 里：上游 release tag 的 `Cargo.lock` 中 workspace 版本仍是 `0.0.0`，cargo 构建时会把它改成真实版本号，这会弄脏开发目录，也会挡住 rebase。

```bash
git -C ~/git/codeks worktree add --detach ~/git/.worktrees/codeks/build codeks/rust-v0.162.1/1
cd ~/git/.worktrees/codeks/build/codex-rs
cargo build --release --bin codex
./target/release/codex --version   # codex-cli 0.162.1
```

换版本时运行 `git -C ~/git/.worktrees/codeks/build checkout --force --detach <tag>`，再构建（增量编译）。

本机的 cargo 如果把 crates.io 换成了镜像，镜像太慢导致超时时，临时改用官方源：`cargo --config 'registries.<镜像名>.index="sparse+https://index.crates.io/"' build --release --bin codex`，镜像名见 `~/.cargo/config.toml`。

fork 构建出的二进制和官方版本号相同。TUI 的升级提示查询的是 openai/codex 的发布，可以把它当作“该同步了”的信号；不要用提示里的命令升级，那样会装回官方版本。不想看到提示，可以在 `config.toml` 里设置 `check_for_update_on_startup = false`。

## npm 包

fork 可以打成 npm 包 `@wacry/codeks`，结构和官方的 `@openai/codex` 一样：一个很小的主包（`bin/codex.js`），加上按平台拆开的平台包。平台包是同一个包名的不同版本，例如 `0.162.1-codeks.4-linux-x64`，主包通过 `optionalDependencies` 别名引用它。目前只构建 linux-x64；不含 voice runtime，所以 TUI 的本地语音模式不可用，app-server 不受影响。细节见补丁 0004。

构建和发布都在 GitHub Actions 上做：

```bash
python3 fork/tools/codeks.py npm                                    # 构建 origin/main 当前的 tag，结果是 artifact
python3 fork/tools/codeks.py npm patch/<slug>                       # 用开发分支试构建，版本号带 dev，不能发布
python3 fork/tools/codeks.py npm codeks/rust-v0.162.1/4 --publish   # 构建并发布到 npm
```

只有 codeks tag 可以发布。重跑会跳过已经发布过的版本。一次构建加发布约 55 分钟，其中编译约 45 分钟（2026-10-15 实测，公开仓库的 4 核 runner）。包页面：https://www.npmjs.com/package/@wacry/codeks 。版本列表里的 `0.0.0-stage` 是 npm 第一次建包时自动加的占位版本，没有 dist-tag 指向它，可以不管。

发布用 npm trusted publishing，仓库里不保存 token。npm 上 `@wacry/codeks` 的 trusted publisher 配置为 GitHub Actions、仓库 `WAcry/codeks`、workflow `codeks-npm.yml`、不限 environment。workflow 用 GitHub 的 OIDC token 向 npm 换取一次性的发布凭据，npm 会给每个版本附带 provenance，说明它由哪次运行、哪个提交构建。

包名里的 scope `@wacry` 是 npm 账号 `wacry`。换包名或重新建包时（`fork/tools/npm_package.py` 里的 `NPM_NAME`，`amend 0000`）：trusted publisher 只能配置给已经存在的包，所以新包要先手动发布一次。可以用 `gh run download` 下载 `codeks.py npm` 构建出的 artifact，在本机 `npm login` 后运行 `npm_package.py publish --dir <目录>`；也可以像第一次那样，临时把 token 存成 secret，在 publish job 里设置 `NODE_AUTH_TOKEN`。之后在 npmjs.com 的包设置里配置 trusted publisher，再删掉临时 token。`npm trust github` 命令也能配置，但它要求账号开启 2FA，不接受 bypass 2FA 的 token。

下游项目用 npm 别名依赖它，代码里的 `@openai/codex` 路径都不用改：

```json
"dependencies": { "@openai/codex": "npm:@wacry/codeks@0.162.1-codeks.5" }
```

主包的 `package.json` 有一个 `codeks` 字段：`tag`、`commit`、`upstreamVersion`、`upstreamTag`、`upstreamCommit`。凡是用包版本号推导二进制版本或上游来源的代码，都要改成读这个字段。例如用包版本号拼出预期的 `codex --version` 输出（`codex-cli <版本>`），换成 fork 包后要改用 `codeks.upstreamVersion`。

全局安装 `npm i -g @wacry/codeks` 也会提供 `codex` 命令，和官方包的命令同名，两者只能装一个。TUI 的升级提示会建议 `npm install -g @openai/codex@latest`，那会装回官方版本，见上一节「构建」最后一段。

## GitHub 仓库设置

这些设置不在 git 里，重建仓库时要重新配置：

- 公开仓库，不是 GitHub fork：独立仓库，通过 `upstream` remote 同步。
- Actions 开启，但上游 workflow 全部停用，只运行 `.github/workflows/codeks-*.yml`。上游 workflow 文件留在仓库里不删，避免每次同步都冲突；停用状态保存在 GitHub 上。`publish`、`pr`、`ci` 和 `npm` 每次推送后都会停用新出现的上游 workflow，并取消它们已经开始的运行。新仓库第一次设置：先在 Actions 关闭的状态下推送 `main`，再运行 `codeks.py setup --github`。
- 别人提交的 PR 不会触发任何 workflow：codeks 自己的 workflow 只能由有写权限的人手动触发，上游 workflow 都已停用。
- PR 只允许 rebase 合并，关闭 merge commit 和 squash（`setup --github` 设置）。正常不在网页上合并，见「用 PR 审查」。
- 不设分支保护：同步需要 force push `main`。
- 只推送 `main`、开发分支和 `codeks/*` tag，不把上游的 `rust-v*` tag 推到 origin。
- npm 发布不需要仓库 secret，凭据来自 npm 上配置的 trusted publisher，见「npm 包」。

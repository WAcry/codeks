# codeks

codeks 是 [openai/codex](https://github.com/openai/codex) 的自用 fork，仓库为 [WAcry/codeks](https://github.com/WAcry/codeks)（私有）。目标是用尽量少的补丁，高频跟随上游发布。

fork 自己的东西都在 `fork/` 目录，文档用中文。上游文件只在补丁里改，每个补丁都有一份文档，写清为什么改、改了什么、上游怎样变化时要扩展或移除它。

## 原则

改动要小、要精炼。能新建文件就不要改热点；能配置或适配层解决就不要分叉业务逻辑。每一处补丁都要能说清：少了它哪条原则或功能会坏。补丁越少，以后和上游合并时冲突越少。

具体怎样落实见 [PATCHING.md](PATCHING.md)。

## 仓库模型

```text
rust-v0.162.1 ── 0000 ── 0001 ── 0002 ── 0003   ← main
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

- 二进制版本号保持上游版本（如 `0.162.1`），不加 fork 后缀。版本号会发给服务端，而 `0.162.1-codeks.1` 这种 semver 预发布版本号会被当成比 `0.162.1` 更旧。
- fork 版本用 tag 区分：`codeks/rust-v0.162.1/2` 表示“基线 rust-v0.162.1 上的第 2 次发布”。基线不变、补丁变了，n 加一；换了基线，n 从 1 重新开始。
- 查看一个版本：`git show codeks/rust-v0.162.1/1`。
- 对比两个版本的补丁：`git range-diff <旧基线>..<旧 tag> <新基线>..<新 tag>`。

## 目录

| 路径 | 内容 |
|---|---|
| `fork/README.md` | 本文：模型、版本、命令、验证、构建、GitHub 设置 |
| `fork/PATCHING.md` | 补丁规范：要不要改、怎样改最小、文档和提交怎么写 |
| `fork/SYNC.md` | 同步上游：流程、冲突处理、同步后审查、回滚 |
| `fork/patches/` | 每个补丁一份文档，`TEMPLATE.md` 是模板 |
| `fork/tools/codeks.py` | 辅助脚本，只依赖 Python 3.11+ 标准库、git 和 gh |

现有补丁可以用 `codeks.py status` 查看，或者直接看 `fork/patches/`。

## 初始化

```bash
git clone https://github.com/WAcry/codeks.git ~/git/codeks
cd ~/git/codeks
python3 fork/tools/codeks.py setup
```

`setup` 可以重复运行。它会添加 `upstream`（openai/codex），并且只拉取上游的 `main` 和 `rust-v*` tag；让 `origin` 拉取 `codeks/*` tag；开启 `rerere`，记住冲突的解决方式，下次自动套用；设置 `pull.ff=only`。

## 命令

在仓库或任意一个 worktree 里运行 `python3 fork/tools/codeks.py <命令>`：

| 命令 | 作用 |
|---|---|
| `status [--fetch]` | 基线、补丁列表、上游有没有新 stable、和 origin 的关系 |
| `check [REV]` | 检查补丁栈和文档是否合规，几秒钟，不编译 |
| `new <slug> <标题>` | 分配下一个补丁编号，从模板生成文档 |
| `amend <ID>` | 把已暂存的改动并入补丁 ID 的提交 |
| `report [TARGET]` | 只读：列出从当前基线到 TARGET 之间与各补丁相关的上游提交 |
| `sync [TARGET]` | 把当前分支的补丁栈 rebase 到 TARGET（默认最新 stable） |
| `publish [-m 说明] [--no-ci]` | 检查后打 tag，推送 `main` 和 tag，触发 CI |
| `ci [分支] [--crates "a b"]` | 推送分支并在 GitHub 上运行 CI |
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
- 完成后：`codeks.py check`，然后 `codeks.py publish`，最后删掉 worktree 和分支。

## 验证与 CI

本地默认不编译、不运行、不测试。补丁通常很小，一般不会破坏上游，而 codex 很大，完整编译和测试代价很高。本地只跑 `codeks.py check`，编译和测试交给 GitHub Actions 上的 `codeks-ci`（补丁 0003）。

- 触发方式：`publish` 推送后自动触发一次；开发分支需要时用 `codeks.py ci`。普通提交和推送都不会触发。
- 检查内容：每次都检查补丁栈和文档。补丁改了 `codex-rs/` 时，再做格式检查、编译 `codex` 和测试辅助程序，并跑补丁涉及的 crate 的测试。
- 成本：私有仓库按分钟计费，标准 runner 只有 2 核。一次 Rust 运行要一个小时左右，没有缓存的第一次更久，所以不要每个提交都跑。

## 构建

只有要用新版本时才在本地构建。构建要放在单独的 worktree 里：上游 release tag 的 `Cargo.lock` 中 workspace 版本仍是 `0.0.0`，cargo 构建时会把它改成真实版本号，这会弄脏开发目录，也会挡住 rebase。

```bash
git -C ~/git/codeks worktree add --detach ~/git/.worktrees/codeks/build codeks/rust-v0.162.1/1
cd ~/git/.worktrees/codeks/build/codex-rs
cargo build --release --bin codex
./target/release/codex --version   # codex-cli 0.162.1
```

换版本时运行 `git -C ~/git/.worktrees/codeks/build checkout --force --detach <tag>`，再构建（增量编译）。

本机的 crates.io 默认走 Artifactory 镜像。镜像太慢导致超时时，临时改用官方源：`cargo --config 'registries.rbx-crates-mirror.index="sparse+https://index.crates.io/"' build --release --bin codex`。

fork 构建出的二进制和官方版本号相同。TUI 的升级提示查询的是 openai/codex 的发布，可以把它当作“该同步了”的信号；不要用提示里的命令升级，那样会装回官方版本。不想看到提示，可以在 `config.toml` 里设置 `check_for_update_on_startup = false`。

## GitHub 仓库设置

这些设置不在 git 里，重建仓库时要重新配置：

- 私有仓库，不是 GitHub fork（GitHub 不允许把公开仓库的 fork 设为私有）。
- Actions 开启，但上游 workflow 全部停用，只运行 `.github/workflows/codeks-*.yml`。上游 workflow 文件留在仓库里不删，避免每次同步都冲突；停用状态保存在 GitHub 上。`publish` 和 `ci` 每次推送后都会停用新出现的上游 workflow，并取消它们已经开始的运行。新仓库第一次设置：先在 Actions 关闭的状态下推送 `main`，再运行 `codeks.py setup --github`。
- 不设分支保护：同步需要 force push `main`。
- 只推送 `main`、开发分支和 `codeks/*` tag，不把上游的 `rust-v*` tag 推到 origin。

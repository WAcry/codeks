# 补丁规范

## 先判断要不要补丁

从上往下找，哪一级能解决就停在哪一级：

1. **不改仓库**：`config.toml`、profile、`model_providers`、环境变量、MCP、skills、插件、hooks、`AGENTS.md` 指令。这样能解决的需求不进 fork。
2. **交给上游**：通用的 bug 或缺失的功能，先查上游的 issue 和 PR。上游已经合入、还没发布的，做回移补丁，不要自己重写。
3. **新文件加最小挂载点**：新逻辑放进新文件（新模块、新 crate、新测试文件），上游文件里只留一两行挂载代码（`mod` 声明、注册调用、一个 match 分支）。
4. **适配层**：在边界上做转换（请求和响应、provider、工具定义），不改业务逻辑本身。
5. **原地修改**：最后才用。改动集中在尽量少的连续行里。

补丁文档的「必要性」要说明为什么前面几级不够用。

## 怎样改得最小

- 一个补丁只做一件事。两件事就拆成两个补丁，即使它们改的是同一个文件。
- 不顺手改：格式化、重排 import、改名、提取函数、修 typo、升级依赖，都会扩大冲突面。
- 不移动上游代码。需要复用就调用；需要不同的行为就在挂载点分流。
- 尽量不加依赖。新依赖要改 `Cargo.toml` 和 `Cargo.lock`，这两个是最热的文件。确实需要时在文档里写明，同步冲突时重新生成锁文件，不手工合并。
- 在每个改动块旁边加一行英文注释作标记，例如 `// codeks 0004: keep tool arguments in plaintext`。用 `rg 'codeks 0004'` 就能找到这个补丁的所有落点，冲突时也能一眼认出来。新文件只在文件头标一次；JSON、锁文件这类不能写注释的文件不加。
- 测试优先放进新文件。必须改上游已有的测试时，只改受影响的断言，并写进文档。
- 不改上游的 `README.md`、`CHANGELOG.md`、`docs/` 和已有的 `.github/workflows/*.yml`。fork 的说明只写在 `fork/`；上游 workflow 通过 GitHub 设置停用（见 README）。
- 代码风格和周边代码保持一致。代码注释用英文，`fork/` 下的文档用中文。

## 判断热点

改上游文件之前，先看它在上游有多活跃：

```bash
git log --since=90.days --oneline upstream/main -- <path> | wc -l
```

次数越多，越应该把逻辑移进新文件、这里只留挂载点，或者换一个更冷的位置挂载。

## 验证

本地默认不编译、不运行、不测试：改动通常很小，而 codex 的完整编译和测试代价很高。

- 本地：`codeks.py check`，几秒钟，不编译。改了 Rust 文件时，可以只对这些文件运行 `rustfmt <文件>`（它会读取 `codex-rs/rustfmt.toml`）。
- 远端：`publish` 后会自动跑 `codeks-ci`。逻辑改动比较大、想在发布前确认时，用 `codeks.py ci` 在开发分支上跑一次。CI 会测试补丁涉及的 crate，见 README「验证与 CI」。
- 文档的「验证」写 CI 之外还要看什么：不需要编译的检查（`rg`、读代码、看配置），或者用上新版本后的手工检查点。

## 编号和文档

- 编号是四位数字，全局递增，永不复用，删掉的补丁的编号也不再用。`codeks.py new` 会自动分配。
- `0000` 留给 fork 自身，只能改 `fork/`。其他补丁只能改上游文件和自己的文档。
- 文档路径是 `fork/patches/NNNN-<slug>.md`。`slug` 用小写英文和连字符，正文用中文，按 [patches/TEMPLATE.md](patches/TEMPLATE.md) 填写：

| 小节 | 写什么 |
|---|---|
| 元数据 | `watch_paths`、`watch_regex`、`backport_of`、`upstream`，供同步报告使用 |
| 背景 | 为什么改：遇到的问题、触发场景、上下文。假设读者没看过当时的讨论 |
| 改动 | 改了什么：行为、逻辑、每一个涉及的路径、关键实现、重要信息 |
| 必要性 | 少了它，哪条原则或功能会坏；为什么更轻的办法不够 |
| 何时扩展 | 上游出现哪些同类变化时，需要增加修改 |
| 何时移除 | 上游出现哪些变化时，可以缩小或删除本补丁 |
| 冲突处理 | rebase 冲突时以什么为准、怎样重新应用 |
| 验证 | CI 之外，还要怎样确认补丁仍然生效 |
| 记录 | 每次实质调整写一行：日期、基线、变化 |

元数据用来让同步报告自动提醒“同类变化”：

- `watch_paths`：补丁自己改过的文件会自动关注，这里写额外要关注的路径（git glob，例如 `codex-rs/core/src/tools/handlers/**`）。
- `watch_regex`：上游 diff 中新增或删除的行匹配时提醒（POSIX 扩展正则，即 `git log -G`）。适合发现“上游又在别处加了同类代码”。
- `backport_of`：回移的上游提交 SHA。新基线包含它，或者包含同标题的 cherry-pick 时，报告会提示删除本补丁。
- `upstream`：相关的上游 issue 或 PR 链接。

`codeks.py check` 会检查：每个提交恰好有一个 `Codeks-Patch` trailer，标题格式正确，编号不重复；文档存在，各小节都有内容，元数据合法；提交里改到的每个上游路径都在文档里出现；补丁提交不碰 `fork/` 里的其他文件。

## 提交

```bash
git add -A
git commit -m "[codeks 0004] <标题>" --trailer "Codeks-Patch: 0004"
```

- 提交标题和文档的一级标题一致（去掉编号）。
- 修改已有补丁用 `codeks.py amend <ID>`，不追加修复提交，保持一个补丁一个提交。
- 补丁之间有依赖时，被依赖的放在下面；没有依赖就按编号排列。

## 回移补丁

上游已经合入、但还没发布的修复：

```bash
python3 fork/tools/codeks.py new backport-<slug> "回移：<上游标题>"
git cherry-pick --no-commit <上游提交>
# 在文档里写 backport_of = ["<上游提交的完整 SHA>"]，背景里写清为什么等不了上游发布
git add -A
git commit -m "[codeks NNNN] 回移：<上游标题>" --trailer "Codeks-Patch: NNNN"
```

新基线包含这个提交之后，同步报告会提示删除它。

# 同步上游

## 什么时候同步

上游发布新的 stable tag（`rust-vX.Y.Z`）后尽快同步，`codeks.py status --fetch` 会提示。补丁少、跟得勤，每次的冲突就小。alpha tag 只在需要其中某个修复、又不适合做回移补丁时才用作基线。

## 流程

```bash
cd ~/git/codeks && python3 fork/tools/codeks.py pull
python3 fork/tools/codeks.py report latest     # 可选：先只读预览
git worktree add -b sync/<目标 tag> ~/git/.worktrees/codeks/sync main
cd ~/git/.worktrees/codeks/sync
python3 fork/tools/codeks.py sync latest        # 也可以指定 tag，例如 rust-v0.163.0
```

`sync` 会：

1. 拉取上游，确认当前补丁栈合规；
2. 生成上游变化报告，打印出来，并保存下来留给 `publish` 写进 tag；
3. 执行 `git rebase --onto <目标> <旧基线>`，只重放 fork 的补丁提交；
4. 成功后打印 `git range-diff` 摘要（`=` 未变，`!` 有调整，`<` 被丢弃），并运行 `check`。

## 冲突处理

rebase 停在某个补丁上时：

1. `git show --stat REBASE_HEAD` 看停在哪个补丁，打开它的文档，按「冲突处理」操作。
2. 以上游的新代码为准，把补丁的意图重新落到新代码上，不要把上游的改动退回旧的样子。
3. 上游已经实现了同样的效果时，用 `git rebase --skip` 丢掉这个补丁，文档会一起丢掉。
4. 补丁有实质调整时，同时更新文档的「改动」「冲突处理」「记录」。
5. `git add <文件>`，然后 `git rebase --continue`。

`rerere` 已经开启：同样的冲突再次出现时，git 会自动套用上次的解决方式，检查后再 `--continue`。想放弃本次同步就 `git rebase --abort`，`main` 不受影响。

全部完成后运行 `codeks.py check`，并用 `sync` 开头打印的 `git range-diff` 命令审查补丁的变化。

## 同步后审查

rebase 没有冲突，不代表补丁还是对的。对照报告，逐个补丁检查：

1. **相关的上游提交**：上游改了补丁改过的文件或 `watch_paths`。读这些提交，判断补丁的行为是否仍然成立。
2. **何时扩展**：对照文档。比如 `watch_regex` 命中，说明上游在别处加了同类代码，补丁要跟着覆盖。
3. **何时移除**：对照文档。上游已经提供同样的能力或配置开关时，删掉补丁，改用配置。回移的提交已经进入基线时，报告会直接提示。
4. **报告开头**：上游改了 `fork/` 或新增了 workflow 时，要额外留意。
5. 文档「验证」里不需要编译的检查顺手做掉；编译和测试交给发布后的 CI。

需要调整的补丁用 `codeks.py amend <ID>` 并入，并在文档的「记录」里加一行。

想让人先审查这次同步，就在同步分支上运行 `codeks.py pr`。同步 PR 的 Files changed 和 Commits 里混有上游的全部变化，没法直接看；PR 描述里有补丁的 range-diff 和同步报告，以它们为准。

## 发布

```bash
python3 fork/tools/codeks.py publish -m "同步到 rust-v0.163.0"
```

`publish` 会再运行一次 `check`，然后：

- 打 annotated tag `codeks/<基线>/<n>`，内容包括基线、补丁列表、与上一版本相比的变化、本次同步报告和 `-m` 的说明；
- 用 `--force-with-lease` 推送 `main` 和 tag，如果这期间 origin 上的 `main` 被别处改过，推送会失败。同步分支创建之后 `main` 又发布过时，`publish` 会直接拒绝：先在新的 `main` 上重新同步；
- 分支开过 PR 时，把分支也推到同一个提交，GitHub 会把 PR 标为 Merged，然后删除远端分支；
- 更新本地 `main`，包括 `~/git/codeks` 这个 checkout（前提是它没有未提交的改动）；
- 停用新出现的上游 workflow，然后触发 `codeks-ci` 并打印运行地址（`--no-ci` 可以跳过）。

CI 失败时，修好相关补丁，`amend` 之后再 `publish`；同一个基线会得到 n+1。

最后清理：

```bash
git -C ~/git/codeks worktree remove ~/git/.worktrees/codeks/sync
git -C ~/git/codeks branch -D sync/<目标 tag>
```

## 回滚

所有发布过的版本都保留在 tag 里。把 `main` 退回某个版本：

```bash
git fetch origin
git push --force-with-lease=refs/heads/main:$(git rev-parse origin/main) origin 'codeks/rust-v0.162.1/1^{commit}:refs/heads/main'
python3 fork/tools/codeks.py pull --force
```

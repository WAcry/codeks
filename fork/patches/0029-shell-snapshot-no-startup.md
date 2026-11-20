# 0029 避免 Shell Snapshot 回放重新读取 Bash 启动配置

```toml
watch_paths = [
  "codex-rs/exec-server/src/shell_snapshot.rs",
  "codex-rs/exec-server/src/shell_snapshot_file.rs",
  "codex-rs/exec-server/tests/exec_process.rs",
  "codex-rs/sandboxing/src/**",
]
watch_regex = ["shell_snapshot_replay_mut|ShellType::Bash|--norc|--noprofile|snapshot_source_capture_script"]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

rust-v0.170.0 的远程 Shell Snapshot 测试揭示：缓存中已经成功捕获 Bash 配置时，回放进程仍可能重新执行 `.bashrc` / 登录初始化文件。在捕获失败后恢复的测试里，`captures` 由预期的两次变为四次；只读沙箱里则出现回放阶段试图写入启动文件的副作用，最终进程退出 41。捕获配置和正常回放应当分离：回放只恢复捕获结果，不能重新执行来源脚本。

## 改动

仅在 `codex-rs/exec-server/src/shell_snapshot.rs::ShellSnapshotCache::prepare` 成功生成回放命令后，为 Bash 的 shell argv 在 `-pc` 前加 `--noprofile` 与 `--norc`，并保持原先 `set +o privileged` 与快照内容恢复脚本不变。参数插入沿用当前 sandbox wrapper 的实际 shell 起点，绝不改变 wrapper 的参数；不改捕获命令、不改快照失效时的回退登录命令，不改变 sh 或 zsh。

## 必要性

只有运行时选择真正的回放命令才能阻止二次执行启动文件；上游现有 `-pc` 与脚本内部取消 privileged 并不足以覆盖现有远程沙箱场景。单独配置 shell 环境或修改测试预期会掩盖 profile 被执行两次的真实副作用。新增两项 Bash 原生标志的影响更窄，且不会修改捕获的 PATH、函数和 alias 数据。

## 何时扩展

上游若改 `ShellSnapshotCache::prepare` 或 sandbox shell argv wrapper，就再次核对标志仍在真正 shell 的 `-pc` 之前。若 shell snapshot 开始支持额外 ShellType，应单独评估对应的启动文件禁用语义，不推导 Bash 标志对其他 shell 可用。

## 何时移除

当上游回放实现已经确保 Bash 不重新读取启动配置，且能通过恢复、缓存和只读沙箱的真实回归测试时，删掉这个补丁。

## 冲突处理

保留上游的缓存键、失败重试和回放方式，只要 Bash 回放确实避免 startup sourcing。若上游用 `WrappedSandboxCommand` 重构了命令参数位置，以真实 Bash argv 的前缀为准，不能直接按固定索引插入额外参数。

## 验证

静态检查完整 argv 顺序（wrapper + bash + `--noprofile --norc -pc` + restore script），运行 `rustfmt --check`、`codeks.py check`。CI 必须执行 `codex-exec-server` 完整测试，包括远程恢复（pipe/tty）和只读沙箱，并留意 `blocked_descriptor_path` 的独立文件描述符禁用问题；不以精确 `ci_skip` 代替行为测试。

## 记录

- 2026-11-19 @ rust-v0.170.0：诊断 CI 的 23 项 Shell Snapshot 测试中，同一试验性禁用 Bash profile/rc 的改动让此前三项远程恢复和只读沙箱测试通过；独立的 `blocked_descriptor_path` 仍因捕获进程非零退出而失败，需要单独定位。当前正式补丁只固定 Bash 回放的 argv，未宣称解决剩余测试。

# 0045 避免 Bash 快照回放重复执行启动配置

```toml
watch_paths = ["codex-rs/exec-server/tests/exec_process.rs", "codex-rs/exec-server/tests/exec_process/shell_snapshot.rs"]
watch_regex = ["shell_snapshot_replay_mut|--noprofile|--norc|ShellType::Bash"]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

远程执行服务成功捕获 Bash 配置后，Bash 回放仍会再次读取 `.bashrc`。恢复测试中应当发生两次的初始化累计发生了四次。只读沙箱中，再次读取 `.bashrc` 触发写入失败，用户命令未能运行。

另一项测试刻意阻止通过文件描述符回放，以覆盖环境变量回放。在 Linux 上，`/dev/fd` 指向 procfs，bubblewrap 试图直接挂载到该符号链接，导致沙箱在启动 shell 前失败。

## 改动

- `codex-rs/exec-server/src/shell_snapshot.rs`：成功准备 Bash 回放时显式禁用 login profile 和 rc，沿用原有 `-pc` 与恢复脚本。捕获与回退仍使用原有启动参数。Zsh 和 POSIX sh 保留已有的参数。
- `codex-rs/sandboxing/src/launch.rs`：复用原有内层 argv 匹配，将启动选项放在 Bash 可执行文件之后、`-pc` 之前。保留外层沙箱启动器、进程权限、网络配置及用户参数。
- `codex-rs/exec-server/src/shell_snapshot_tests.rs`：比较并发回放时，在末尾脚本参数中归一化每个命令自己的文件描述符，并检查 Bash 启动选项与回退时的原始命令。
- `codex-rs/exec-server/tests/common/mod.rs` 和 `codex-rs/exec-server/tests/exec_process/shell_snapshot.rs`：Linux 描述符用例仅在测试进程使用既有 `--no-proc` 参数，避免新的 procfs 挂载覆盖测试的 `/proc` 禁读规则。macOS 继续使用 `/dev/fd` 禁读规则。通过函数的来源断言区分环境变量与描述符回放，继续检查一次捕获及并发输入、输出和退出状态。

## 必要性

启动文件在恢复脚本运行之前就可能执行。Bash 必须在进程参数中收到禁用选项，才能把读取启动配置的行为限制在捕获和普通启动路径。

## 何时扩展

上游更改回放的 argv 匹配、包装器或 Bash 参数时，检查附加选项仍位于真正的 shell 参数内，并在成功回放时才生效。

## 何时移除

上游回放实现已覆盖远程 Bash 的自动启动文件行为，且恢复与只读沙箱测试通过时，删除对应改动。

## 冲突处理

保留上游的沙箱包装器和捕获策略。回放继续使用经过验证的内层 argv，捕获失败时按原规则恢复凭据与执行原命令。

## 验证

由 `codeks-ci` 的 `codex-exec-server` 测试运行远程 pipe、PTY 恢复、只读沙箱及描述符禁读用例，确认捕获次数、回放来源、并发输入和命令结果。

## 记录

- 2026-11-20 @ rust-v0.171.0：接入新版 `WrappedSandboxCommand`，将 Bash 专用启动选项限制在成功回放路径。
- 2026-11-20 @ rust-v0.171.0：适配并发回放单元测试的脚本位置，继续校验捕获次数、回退参数及重试上限。
- 2026-11-20 @ rust-v0.171.0：让描述符回退测试使用稳定的 Linux 挂载目标，并验证实际回放来源。

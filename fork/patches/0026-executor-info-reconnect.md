# 0026 执行服务重启后恢复环境信息查询

```toml
watch_paths = ["codex-rs/app-server/src/request_processors/environment_processor.rs", "codex-rs/exec-server/src/client_recovery.rs", "codex-rs/exec-server/src/client_transport.rs"]
watch_regex = ["force_environment_info|force_info|RecoveryPolicy|unknown session id"]
backport_of = []
upstream = ["https://github.com/openai/codex/commit/de80fa6e3194d68b71b0f09be475179922e0f5b8"]
ci_skip = []
```

## 背景

执行服务重启会失去原有 session，断线超过保留期也会使 session 过期。控制端可能仍持有正在恢复的旧连接。此时客户端连接探测调用 `environment/info`，旧 session 恢复被拒绝，第一次请求报错；第二次相同请求才取得新 session 并成功。关闭后重新连接是常用操作，每次需要手动重试的体验不合理。

查询本身也可能先发出，再遇到断线；只在取得连接前等待恢复，仍会留下这个窗口。已有命令的执行结果可能不确定，但环境元数据是只读查询，可以在连接恢复结束后再读取一次。

## 改动

- `codex-rs/exec-server/src/client.rs`：只注册独立 metadata 适配模块及专属测试模块，不改变通用 `get()`、命令、文件和 HTTP 调用。
- `codex-rs/exec-server/src/client_environment_info.rs`：新增 `LazyRemoteExecServerClient::force_environment_info()`。首次取得连接失败直接返回；仅对等待恢复模式下、允许重新连接的传输，处理 info 的 `Disconnected`。先等待已有原生恢复结束，再取得当前连接并重读一次。恢复成功时复用原 session，终止失败时由原生 lazy client 创建新 session。其他错误直接返回，第二次读取失败也直接返回。
- `codex-rs/exec-server/src/environment.rs`：`Environment::force_info()` 的远端分支调用上述入口，本地查询保持原实现。
- `codex-rs/exec-server/src/client/tests/codeks_environment_info_tests.rs`：使用真实 WebSocket fixture 验证只读重连边界。

不解析错误字符串，不新增 RPC、超时、退避、配置开关或进程恢复协议。FailFast 查询、策略错误、普通 RPC 错误保持原语义。原有进程句柄和在途命令继续使用原 client；没有把命令或文件写入重发到新 session。

## 必要性

配置不能修复客户端取得旧连接后发生的断线窗口。由调用端匹配错误文字并重试，会让每个客户端重复理解原生 session 生命周期；仅延长执行服务进程寿命也不能覆盖服务升级、崩溃和会话过期。用户确认该问题在常用的断开再连接流程中反复发生，因此选择在原生只读探测边界做小补丁。

新逻辑集中于独立模块，原文件只留注册和调用。通用连接取得逻辑、原生 session 恢复、会话保留期以及命令失败机制均不改。已检查上游当前实现，尚无等价修复可以直接回移；相关上游旧提交也要求新操作可取得新连接，而旧操作明确失败、不重放。

## 何时扩展

上游移除 `force_environment_info`、将 app-server 改为另一元数据入口，或改变 `Disconnected`、原生恢复完成和策略拒绝的错误类型时，检查挂载点和边界是否仍适用。保留只读、单次重读的范围，不扩大到通用 RPC 重试。

## 何时移除

上游原生环境元数据入口已能在旧 session 恢复终止后取得新连接、于同一次只读查询完成探测，同时保留 FailFast、策略错误和旧进程失败语义时，删除整个适配模块、两个挂载点及专属测试。若上游先提供等价的安全读取函数，则直接调用它并缩小补丁；不保留双重重试。

## 冲突处理

以新基线的原生连接恢复、超时和错误类型为准。不要将补丁并入通用 `get()`，不要修改命令请求或把旧进程句柄迁移到新 session。原文件只恢复带 `codeks 0026` 标记的模块注册与读取入口调用；上游已满足条件时直接删除。

## 验证

本地仅做相关 Rust 文件格式检查、`git diff --check` 和 `codeks.py check`，不编译或运行 Rust 测试。正常远端 CI 执行新增的 WebSocket 测试以及执行服务相关测试，无 `ci_skip`。

发布后用现有执行服务验证：正在使用的 info 请求遇到断线，旧 session 恢复失败后，同一次请求成功取得新元数据；短断线恢复仍保留 session。停止并重启执行服务后，首次重新连接即可成功。验证普通 RPC 错误不产生额外连接，第二次失败原样返回，旧命令不重发。该修复在控制端，无需同步更新独立执行服务。

## 记录

- 2026-11-11 @ rust-v0.167.1：在原生只读环境信息查询边界支持单次恢复后重读。
- 2026-11-11 @ rust-v0.167.1：按远端 CI 结果显式导入测试断言宏，保持生产行为和测试断言不变。

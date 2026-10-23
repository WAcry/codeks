# 0010 恢复和分叉线程时显式选择执行环境

```toml
watch_paths = ["codex-rs/core/src/thread_manager.rs", "codex-rs/core/src/session/session.rs", "codex-rs/app-server/src/request_processors.rs", "codex-rs/app-server-protocol/src/precomputed_exports*"]
watch_regex = ["ThreadResumeParams|ThreadForkParams|resolve_turn_environment_requests|environment_selections|selected_environments"]
backport_of = []
upstream = ["https://github.com/openai/codex/pull/20520", "https://github.com/openai/codex/pull/43848", "https://github.com/openai/codex/pull/42386"]
ci_skip = []
```

## 背景

一个 app-server 可以注册多个执行环境，`thread/start` 和 `turn/start` 也能明确选择环境。但 0.164.0 的 `thread/resume` 和普通 `thread/fork` 不能接收该选择，冷恢复会按进程默认值初始化。会话初始化会建立 MCP 等能力；已有 Goal 也可能自动继续。因此，客户端等到下一次 `turn/start` 才恢复选择已经太晚。

需要让持有线程绑定的客户端，在冷恢复和普通 fork 初始化之前传入执行环境。省略参数的调用继续使用上游行为。

## 改动

在两个请求中加入可选实验字段 `environments`，类型和语义复用 `thread/start` 的 `TurnEnvironmentParams`：省略保留原有选择规则，空数组明确不选择执行环境，非空数组明确指定环境及工作目录。使用现有解析器校验环境 ID、路径和工作区根目录，并通过 `StartThreadOptions` 在 Session 初始化前传入。

对已加载线程，显式选择必须与现有环境 ID、工作目录和根目录一致；不一致直接返回错误，避免客户端误认为已经换了执行环境。冷恢复由父线程管理的子代理时，显式参数被拒绝，省略参数仍走上游的父线程继承路径。预测 fork 保留原有继承规则，不接受该参数。

涉及路径：

- `codex-rs/app-server-protocol/src/protocol/v2/thread.rs`：两个实验字段。
- `codex-rs/app-server/src/request_processors/thread_processor.rs`：恢复和 fork 的参数传递、子代理和预测 fork 边界，以及已加载线程的检查挂点。
- `codex-rs/app-server/src/request_processors/thread_environment_overrides.rs`：已加载线程的环境选择比较。
- `codex-rs/app-server/src/request_processors/thread_processor_tests.rs`：现有完整参数构造补上新字段。
- `codex-rs/app-server/tests/suite/v2/mod.rs` 和 `codex-rs/app-server/tests/suite/v2/thread_environment_overrides.rs`：协议集成测试。
- `codex-rs/app-server-protocol/schema/json/v2/ThreadResumeParams.json` 和 `codex-rs/app-server-protocol/schema/json/v2/ThreadForkParams.json`：稳定 JSON 导出保留生成器引入的类型定义，但隐藏实验字段。
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-stable.json.zst` 和 `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-experimental.json.zst`：同步预计算导出，实验 TypeScript 和 JSON 包含新字段；稳定 TypeScript 与内部 rollout schema 不变。

此补丁不持久化线程绑定、不重新注册环境，也不改变连接重试、Goal 或执行请求重放。调用方仍需保存绑定，在进程重启后注册环境，并在恢复和 fork 请求中明确传入。不存在的环境不会退回默认环境；已注册但不可达的环境仍由原有连接层报告错误。

选择参数只检查环境是否已注册及路径语法，不承诺网络可达。需要在启动前判断连通性的调用方应先使用 `environment/info`，并仍然处理后续断线。已选 Skills 根目录由上游会话元数据恢复，此补丁不增加另一份存储。

## 必要性

用户已允许采用最小原生补丁，并要求比较覆盖范围与维护成本。只用进程默认环境配置不能满足同一进程中不同线程选择不同机器；为每个环境启动独立 app-server 会增加身份、历史和进程管理成本。模型协议 bridge 不管理线程初始化和执行环境，无法修复这个时序。

完整持久化上游环境选择需要定义历史 fork、回滚、空选择、初始状态和父子线程的恢复规则，超出了当前缺失的请求参数。这里复用已经存在的启动选项，保留默认行为，只补客户端明确表达选择的入口。直接使用 CLI 或不传入字段的客户端，不会因此自动获得持久化绑定。

## 何时扩展

上游改变 `StartThreadOptions`、环境解析器、已加载线程的配置快照或父子线程继承时，重新检查参数是否仍在 Session 初始化之前生效。新增恢复入口需要判断其是否已经继承正确环境，不能直接把客户端覆盖规则套进父线程控制的子代理。若要覆盖独立 CLI 或服务端自主恢复，应单独评估原生持久化规则。

## 何时移除

上游提供同等的 resume/fork 参数，或原生持久化能够在首次能力初始化和 Goal 继续之前恢复正确环境时，删除对应挂点。PR #20520 曾尝试持久化环境选择，但调查时已关闭且未合并；#43848 只持久化工作区根目录，#42386 只返回环境选择，均不能替代本补丁。

## 冲突处理

保留上游新的线程生命周期和锁顺序，只重新接入两个字段、启动选项和已加载线程检查。不要复制整个恢复实现。若上游提供了等价字段，优先删除补丁而非保留同义参数。协议导出以对应版本生成器为准；预计算导出必须同时保持实验接口可见、稳定接口隐藏。

## 验证

本地运行 `codeks.py check`、`git diff --check`，仅对修改的 Rust 文件运行 rustfmt。编译和协议集成测试通过远端 `codeks-ci` 的 `codex-app-server codex-app-server-protocol` 两个 crate 执行。

使用新构建时，注册本地和远端环境，显式选择远端创建线程；重启 app-server 后重新注册环境，带同一选择冷恢复并普通 fork。检查首次执行和 Goal 自动继续所在的环境；检查未知环境返回错误、空数组仍为空，以及已加载线程不能被带不同选择的 resume 悄悄改动。另用 `generate-ts --experimental` 确认两个参数含有字段，稳定导出仍不含该字段。

## 记录

<!-- 每次实质调整写一行，最新的放在最下面：- YYYY-MM-DD @ <基线>：变化 -->

- 2026-10-23 @ rust-v0.164.0：新增。

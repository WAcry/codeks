# 0007 自动压缩：服务端 usage 一律视为已包含历史 reasoning

```toml
watch_paths = []
watch_regex = [
  "get_non_last_reasoning_items_tokens",
  "server_reasoning_included",
  "ServerReasoningIncluded",
  "[Rr]easoning-[Ii]ncluded",
  "get_total_token_usage",
]
backport_of = []
upstream = [
  "https://github.com/openai/codex/issues/39767",
  "https://github.com/openai/codex/issues/49026",
  "https://github.com/openai/codex/issues/49961",
  "https://github.com/openai/codex/issues/32483",
  "https://github.com/openai/codex/pull/55991",
]
```

## 背景

自动压缩用 `Session::get_total_token_usage()` 估算当前上下文有多大，再和压缩阈值比较。这个值由三部分组成（`codex-rs/core/src/context_manager/history.rs` 的 `ContextManager::get_total_token_usage`）：

1. 上一个响应的 usage 里的 `total_tokens`，来自服务端。
2. 上一个模型输出之后本地新加的条目（工具输出、用户消息）的估算值。
3. 服务端“没有包含历史 reasoning”时，再加上最后一条用户消息之前所有带密文的 reasoning 条目的估算值（`get_non_last_reasoning_items_tokens`）。

第 3 部分的来历：2025-11 的 #7113 认为 API 返回的 usage 不含之前的 reasoning，于是在客户端补算；2026-01 的 #9402 加了响应头 `x-reasoning-included`，服务端带这个头时才不补算。

实际情况是：服务端返回的 usage 总是已经算上了之前的 reasoning，而服务端都不发这个头。我们用到的服务（Azure、AWS 等）都不发；#49026 和 #49961 里有人直接测过官方的 Codex Responses 接口和 ChatGPT 的 Codex 后端，响应里也没有这个头。#49026 的对照实测（gpt-6.1-sol）里，带上历史 reasoning 后 input usage 从 9 涨到 50，服务端明确把多出的 41 算给了历史 reasoning，两次响应都没有这个头。于是同一批 reasoning 被算了两次，自动压缩提前触发：

- #49961（Codex 0.162.1，gpt-6.1-sol，xhigh）：服务端报告 176,426，补算历史 reasoning 61,645，新工具输出 10,048，合计 248,119，超过阈值 244,800。界面显示上下文只用了 67%，压缩已经开始。
- #39767：重放 76 次自动压缩，按现在的补算逻辑 76 次都到了阈值；不补算历史 reasoning 时，76 次都没到。

0.164.0 的上游 #55991 已修复 Responses Lite：普通采样和本地压缩前按 `model_info.use_responses_lite` 初始化标志，Lite 不再依赖响应头；本地重新估算 usage 后也标成已包含 reasoning。每个 turn 开头重置为 false 的逻辑也已取消。但非 Lite 请求仍初始化为 false，没有响应头时仍补算。本补丁继续覆盖这一条路径，不把“usage 已含 reasoning”的原则绑定到协议版本。

旧模型会在服务端丢掉之前 turn 的 reasoning。这时 usage 不含它们，它们也不占上下文，同样不该补算。所以不管哪种模型、有没有这个头、头的值是什么，服务端的 usage 都可以直接当作上下文大小。

## 改动

`codex-rs/core/src/state/session.rs` 的 `SessionState::server_reasoning_included()` 恒返回 true（常量 `USAGE_INCLUDES_REASONING`），相当于每个响应都带了 `x-reasoning-included: true`。它是 core 里唯一读取这个标志的地方，所以 `ContextManager::get_total_token_usage` 总是走“已包含”的分支：上下文大小 = 上一个响应的 `total_tokens` + 之后本地新加条目的估算值。

受影响的调用方都经过 `Session::get_total_token_usage()`：

- 自动压缩：turn 开始前和 turn 中间的检查（`codex-rs/core/src/session/context_window.rs`），切换到更小上下文的模型时的检查（`turn.rs` 的 `maybe_run_previous_model_inline_compact`）。
- 压缩前后的上下文大小记录（`compact.rs`、`compact_remote_v2.rs`）。
- Guardian 的输入预算（`guardian/input_budget.rs`、`guardian/review_session.rs`）。它们取这个值和本地完整历史估算值中较大的一个，所以补丁只会让它变小或不变，仍然不会低于本地估算。

响应头的解析、`ServerReasoningIncluded` 事件和请求前的标志初始化都保持上游原样。setter 只更新字段，getter 始终返回 true，所以非 Lite 请求把字段设成 false 也不会重新启用补算；`codex doctor` 仍然显示服务端有没有发这个头。界面上的上下文百分比本来就只用服务端的 usage，不受影响；改完后压缩时机和界面显示的用量对得上。

## 必要性

少了它，非 Lite 请求在不发这个头的服务上仍会重复补算，长会话可能提前压缩。压缩会丢掉细节，还要多花一次请求的时间和 token。当前 Lite 路径已由上游修复，但我们要求所有协议都直接信任服务端 usage，因此仍保留这个小补丁。

- 配置不够：调高 `model_auto_compact_token_limit` 只能抵消一个固定的量，而补算量随历史 reasoning 的多少变化。调高后 reasoning 少的会话会压缩得太晚，可能超出上下文窗口。
- 适配层可以在响应里补上这个头，但要维护代理，并保证所有直连和代理请求都经过它。本补丁在客户端统一生效，不增加服务部署或配置负担。
- 改在读取标志的 getter 里：一处 3 行，所有调用方自动生效。`state/session.rs` 是三个候选位置里最冷的（上游 90 天内的提交：`state/session.rs` 22 次，`context_manager/history.rs` 66 次，`session/mod.rs` 312 次）。

## 何时扩展

- 上游新增绕过这个 getter 的调用，例如直接调用 `ContextManager::get_total_token_usage(false)`，或者自己传一个标志：让它也按“已包含”处理。watch_regex 里的 `get_total_token_usage`、`server_reasoning_included` 会提醒。
- 上游在别处加了同类的补算（客户端再估算一遍服务端 usage 已经算过的内容，用某个响应头决定要不要算）：先和用户确认，再决定是否扩展本补丁。

## 何时移除

- 上游删掉 `get_non_last_reasoning_items_tokens` 这份补算，或者改成默认不补算、按模型能力或配置决定，并且不发这个头的服务也不再补算：删掉本补丁。上面几个 issue 关闭时检查一下修复方式。
- 上游只是让更多接口发这个头，或只让 Lite 默认不补算：不够。非 Lite 且没有响应头的兼容服务也必须不再补算，才能覆盖本补丁的完整行为。

## 冲突处理

规则只有一条：判断要不要补算历史 reasoning 的地方，一律当作服务端已包含。冲突时以上游代码为准，把这条规则重新套到读取标志的位置。上游把标志改名或移动时，跟着移过去；上游删掉了补算，就删掉本补丁。用 `rg 'codeks 0007'` 找到落点。

## 验证

- 不编译的检查：`rg 'codeks 0007' codex-rs` 有 1 处；`rg -n 'server_reasoning_included\(\)|get_total_token_usage\(' codex-rs/core/src` 里，`ContextManager::get_total_token_usage` 只被 `SessionState::get_total_token_usage` 调用，后者只被 `Session::get_total_token_usage` 调用。多出来的调用方要按「何时扩展」检查。
- 上游 `suite::compact::auto_compact_accounts_for_encrypted_reasoning` 验证 Lite 无响应头时，usage 为 80 不触发阈值 81，usage 为 81 时触发压缩。该测试与补丁行为一致，应正常进入远端 CI。非 Lite 的必要性通过初始化为 false、无响应头事件和 getter 恒 true 的调用链核对。
- 用上新版本后：在 Azure 的 gpt-5.6-sol 或 AWS 的 gpt-6.1-sol 上用高 reasoning 跑一个长会话，自动压缩应该在界面显示的上下文用量接近阈值时才发生。也可以看会话记录里压缩前最后一个 `token_count` 事件：`last_token_usage.total_tokens` 加上之后新增的工具输出，应该接近压缩阈值。

## 记录

- 2026-10-16 @ rust-v0.162.1：新增。
- 2026-10-23 @ rust-v0.164.0：上游 #55991 修复 Lite 和本地重估 usage 的重复补算，并取消 turn 开头的标志重置；非 Lite 无响应头仍有补算，因此保留 getter 补丁，运行时改动不变。同步更新必要性和验证说明。

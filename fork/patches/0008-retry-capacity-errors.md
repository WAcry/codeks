# 0008 容量错误：没有 Retry-After 时也退避重试

```toml
watch_paths = [
  "codex-rs/codex-api/src/api_bridge.rs",
  "codex-rs/codex-api/src/sse/responses_error.rs",
  "codex-rs/core/src/responses_retry.rs",
  "codex-rs/core/src/session/async_compaction.rs",
  "codex-rs/codex-client/src/retry.rs",
]
watch_regex = [
  "server_is_overloaded",
  "CodexErrorDetails::(ServerOverloaded|RetryLimit)",
  "retry_429",
  "MAX_REMOTE_COMPACTION_V2_STREAM_RETRIES",
  "(DEFAULT|MAX)_(STREAM|REQUEST)_MAX_RETRIES",
]
backport_of = []
upstream = [
  "https://github.com/openai/codex/issues/22390",
  "https://github.com/openai/codex/issues/50959",
  "https://github.com/openai/codex/issues/53862",
  "https://github.com/openai/codex/issues/53594",
  "https://github.com/openai/codex/pull/46540",
  "https://github.com/openai/codex/pull/47641",
  "https://github.com/openai/codex/pull/23951",
]
ci_skip = [
  # 断言“容量错误没有可用的 Retry-After 时直接失败”的上游测试。本补丁有意让它们按本地退避重试，见「改动」
  "package(codex-protocol) & test(=error::tests::retryability_preserves_error_details_distinctions)",
  "package(codex-core) & test(=suite::retry_after::responses_http_overload_respects_retry_limits::without_advice)",
  "package(codex-core) & test(=suite::retry_after::sse_overload_with_retry_after_is_terminal)",
  "package(codex-core) & test(=suite::retry_after::sse_overload_without_retry_after_is_terminal)",
  "package(codex-core) & test(=suite::retry_after::websocket_rate_limit_without_retry_after_is_terminal)",
  "package(codex-core) & test(=suite::retry_after::websocket_overload_without_retry_after_is_terminal)",
  "package(codex-core) & test(=suite::retry_after::websocket_overload_retries_only_while_it_has_retry_after)",
  # 远端压缩遇到没有 Retry-After 的 503 overload 后，从直接失败变成按 stream_max_retries 重试（这个测试里是 2 次）
  "package(codex-core) & test(=suite::retry_after::compact_v2_overload_without_retry_after_exhausts_request_retries)",
  # 审批审查会话里的流内 overload 先在会话内部重试成功，外层审查只用了 1 次尝试（测试期望 2 次）
  "package(codex-core) & test(=guardian::tests::guardian_review_retries_transient_session_failure_then_approves)",
]
```

## 背景

上游服务容量不够时，常见三种错误：

1. HTTP 429，但不是配额或额度用完（限流、容量不足），而且没有 `Retry-After`。
2. HTTP 503，body 里 `error.code = "server_is_overloaded"`，没有 `Retry-After`。
3. 流已经开始后，`response.failed` 事件里 `error.code = "server_is_overloaded"`，没有 `Retry-After`。

它们分别被映射成 `CodexErrorDetails::RetryLimit`（`codex-rs/codex-api/src/api_bridge.rs`，429 在排除各种配额错误之后的兜底分支）和 `CodexErrorDetails::ServerOverloaded`（同文件的 503 分支，以及 `codex-rs/codex-api/src/sse/responses_error.rs`）。

turn 层的重试（`codex-rs/core/src/responses_retry.rs` 的 `handle_response_stream_error`）只看 `CodexErr::retry_delay` 返回不返回延迟。上游在 #46540、#47641 把重试决策集中到这个函数后，这两类错误只有服务端给了 `Retry-After` 才重试，否则返回 `None`，turn 直接失败（界面显示 “Selected model is at capacity…”），`stream_max_retries` 配多大都没用。HTTP 层也补不上：`codex-rs/model-provider-info/src/lib.rs` 里 `retry_429: false` 是写死的，不能配置；503 在 HTTP 层只会安静地重试 `request_max_retries` 次（默认 4 次，共约 3 秒）；流内错误不经过 HTTP 层。

前台远端 V2 压缩（`codex-rs/core/src/compact_remote_v2.rs`，OpenAI 和 Azure Responses 默认使用）还多一层限制：上游在 #23951 把它的流层重试截成 `min(stream_max_retries, 2)`（常量 `MAX_REMOTE_COMPACTION_V2_STREAM_RETRIES`），理由是一次压缩请求可能比普通 turn 跑得久得多。这个上限对所有可重试的错误都生效，包括上游本来就会重试的流内 `rate_limit_exceeded` / `slow_down`（`CodexErrorDetails::RateLimitExceeded`）。自动压缩通常发生在长任务做到一半、上下文快满的时候；容量不足赶上压缩时，普通采样能等很久，压缩却在大约 10 秒内结束整个 turn。

我们有很多 API 用户共用上游服务，容量不足时有时持续很久（例如一小时）。这时 turn 直接失败，用户只能手动重发；长时间无人值守的任务（子代理、`/goal`）会停在半路。上游也有人报告同样的问题（见 `upstream` 里的 issue），都还没修。

## 改动

两处改动，目标相同：容量和限流错误进入普通重试，受 `stream_max_retries` 控制，界面显示 “Reconnecting... n/m”，用户可以随时中断。

1. `codex-rs/protocol/src/error.rs` 的 `CodexErr::retry_delay` 里，`ServerOverloaded | RetryLimit(_)` 这一个分支：有 `Retry-After` 时仍然按服务端的时间等；没有时改用和其他可重试错误相同的本地退避 `backoff(retry_count)`（200ms × 2^(n-1)，±10% 抖动，不设上限）。
2. `codex-rs/core/src/compact_remote_v2.rs` 的常量 `MAX_REMOTE_COMPACTION_V2_STREAM_RETRIES` 从 2 改成 `u64::MAX`。上游用它算 `min(stream_max_retries, 2)`，改完以后结果就是 provider 的 `stream_max_retries`：前台远端 V2 压缩和普通采样用同一个重试次数，对所有可重试的错误都一样，包括上面两类和 `RateLimitExceeded`。取最小值的那行代码和共用的重试处理函数都不改。

影响范围：

- 普通 turn 和子代理的采样请求：上面三种错误都会重试，用完 `stream_max_retries` 后才失败。
- 配额类错误不受影响：`usage_limit_reached`、`usage_not_included`、`insufficient_quota` 等在 `RetryLimit` 兜底之前就映射成了别的错误，仍然直接失败。
- 前台远端 V2 压缩：所有可重试的错误（上面三种、流内 `rate_limit_exceeded` / `slow_down`、断流、空闲超时、5xx 等）都按 `stream_max_retries` 重试，和普通采样一样。断流或空闲超时的一次尝试最长要等 `stream_idle_timeout_ms`（默认 5 分钟），压缩卡住时放弃前会等很久，见「必要性」里的代价。
- 后台远端 V2 压缩：0.163.0 新增的 `[features] async_compaction` 默认关闭。开启后会预先发起 HTTP 压缩请求，保留底层 `request_max_retries`，但不进入前台的流层重试循环；失败结果由 `session/async_compaction.rs` 丢弃，不直接终止 turn。实际需要压缩而没有可用后台结果时，仍走上面的前台路径。本补丁保留这个投机请求的行为；启用后台压缩或扩展其重试前，先和用户讨论。
- 本地压缩（`codex-rs/core/src/compact.rs`）：上游本来就对几乎所有错误按 `stream_max_retries` 退避重试，不经过 `retry_delay`，不受影响。
- 不完整响应：0.163.0 的 `ResponseIncomplete` 是终止错误，例如 `response.incomplete` 的 `max_output_tokens` 或未知原因；`content_filter` 和 `interrupted` 保持各自的处理。普通采样、前台远端和本地压缩都不重试这类错误，压缩也不切换模型重试。它不属于容量错误，本补丁保留上游行为。
- 提供方认证恢复（`codex-rs/core/src/client.rs` 里调用 `retry_delay(1)` 的地方）：恢复过程本身遇到这两类错误时，改为返回原来的 401 错误。这和其他可重试错误的处理一致。
- 自动审批审查（guardian）：审查会话内部的 turn 先按上面的规则重试；外层审查自己的重试和 90 秒超时不变。
- 元数据 `ci_skip`：9 个上游测试断言的正是旧行为（这两类错误没有可用的 `Retry-After` 时直接失败，或者只重试到建议消失为止），本补丁之后一定失败。其中 `sse_overload_with_retry_after_is_terminal` 把 `Retry-After` 放在外层 200 响应上，上游不把它用于流内错误，所以对这个错误来说也是“没有建议”。CI 跳过它们（见 0003），不改上游测试文件。

推荐配置（源码里的默认值不改，按需写进自己的 `config.toml`）：

| 配置项 | 上游默认 | 推荐 | 说明 |
| --- | --- | --- | --- |
| `stream_max_retries` | 5（最大 100） | 14 | 流层重试次数，会显示进度，用户可以随时中断。14 次大约等 55 分钟，能撑过一小时级别的容量故障；要更长就用 15（约 1 小时 49 分） |
| `request_max_retries` | 4（最大 100） | 4（不改） | HTTP 层重试，只管请求还没建立流时的 5xx 和连接失败，不重试 429，也不显示进度。它嵌在流层的每一次尝试里，调大只会让每次尝试多安静地等一会儿；总时长用 `stream_max_retries` 控制 |
| `stream_idle_timeout_ms` | 300000 | 不改 | 流多久没有数据才判定断开，和容量错误无关 |
| `supports_websockets` | 自定义 provider 为 false | 不改 | 开启后，流层重试用完会切到 HTTPS 并把计数清零，总时长大约翻倍 |
| `[features] unbounded_connection_retries` | 开启 | 不改 | 普通采样遇到连接失败时一直等网络恢复（间隔最长 60 秒），不受 `stream_max_retries` 限制；远端压缩不走这条路 |

前四项写在 `[model_providers.<id>]` 里，只对自定义 provider 生效。内置 provider（例如 `openai`）不能在配置里覆盖，同名的配置会被忽略（`codex-rs/model-provider-info/src/lib.rs` 的 `merge_configured_model_providers`）。

```toml
[model_providers.azure-responses]
# ...
stream_max_retries = 14
```

不设上限时，第 n 次重试前等 0.2 × 2^(n-1) 秒，放弃前累计等待 0.2 × (2^N − 1) 秒：

| `stream_max_retries` | 最后一次等待 | 放弃前累计等待 |
| --- | --- | --- |
| 5（默认） | 3.2 秒 | 6.2 秒 |
| 10 | 1.7 分钟 | 3.4 分钟 |
| 13 | 13.7 分钟 | 27.3 分钟 |
| 14 | 27.3 分钟 | 54.6 分钟 |
| 15 | 54.6 分钟 | 1 小时 49 分 |

503 的每一次流层尝试里，HTTP 层还会先安静地重试 `request_max_retries` 次（默认 4 次，约 3 秒），所以 `stream_max_retries = 14` 时最多发 75 个请求。调大以后，压缩也会等得更久：本地压缩（AWS 等不用远端压缩的 provider）上游本来就对几乎所有错误按 `stream_max_retries` 重试，前台远端 V2 压缩在本补丁之后也一样。

## 必要性

少了它，上游容量不足又没给 `Retry-After` 时，turn 和子代理直接失败，需要人手动重发；长任务会停在半路。

- 配置不够：`stream_max_retries` 只对 `retry_delay` 返回延迟的错误生效；429 在 HTTP 层的重试开关 `retry_429` 是写死的。
- 适配层不够：代理可以给 HTTP 响应补 `Retry-After`，但流内的 `response.failed` 已经在 200 响应的流里，代理要解析并改写 SSE 事件，还要多部署一个服务。
- 前台远端 V2 压缩的上限：不放开的话，上面的修改到压缩这一步就失效，容量不足赶上压缩时 turn 仍然在约 10 秒内失败。我们选择整个去掉上限：只放开容量和限流错误，要在重试循环里加约 10 行按错误类型判断的代码，落在上游常改的函数里；去掉上限只改常量这一行。上游截成 2 次的理由是一次压缩请求可能跑很久，我们接受压缩等得更久。也可以用 `[model_providers.<id>.capabilities] remote_compaction = "unsupported"` 改成本地压缩，不用补丁，但压缩方式整个变了（由模型自己写摘要），代价更大。
- 改在 `retry_delay`：这是上游集中做重试决策的地方，所有 turn 层调用方自动生效，改动只有一个分支的函数体。`responses_retry.rs` 更冷（上游 90 天内 11 次提交，`error.rs` 25 次），但在那里改要在调用方重新判断错误类型，等于把分类逻辑分叉出去。

代价：有些不会自己恢复的错误也会重试到次数用完才失败，采样和前台远端 V2 压缩都一样。`stream_max_retries = 14` 时，最多白等约 55 分钟：

- 形状没被上游识别的配额类 429，会被当成容量问题。
- `server_is_overloaded` 不一定是临时容量问题：上游测试里有它配 “This model is disabled.” 的例子，上游的提示语也是让用户换模型（#11388 把原来的 model cap 错误改成了 server overload）。模型被停用时也会一直重试。

前台远端 V2 压缩卡住（流一直没有数据）时更久：每次尝试都要等满 `stream_idle_timeout_ms`（默认 5 分钟），15 次尝试加上退避，放弃前最多大约 2 小时 10 分。用户可以随时中断。

## 何时扩展

- 上游的相关变化（例如新增容量类错误码或错误类型、改动 429/503 的映射、调整 turn 层的重试决策），以及其他值得重试却直接失败的错误：先和用户讨论，再决定是否扩展本补丁。watch_paths 和 watch_regex 会提醒。
- 上游新增其他有自己重试上限的请求路径（例如后台或预先发起的远端压缩），或者把这个上限挪到别处：先和用户讨论，再决定是否套用同样的规则。

## 何时移除

- 上游让 `ServerOverloaded` / `RetryLimit` 在没有 `Retry-After` 时也按本地退避重试，或者提供了等价的配置开关：删掉 `error.rs` 那一处。`upstream` 里的 issue 关闭时检查修复方式。
- 上游去掉前台远端 V2 压缩的 2 次上限，或者改成按错误类型分配重试次数、让容量和限流错误用 `stream_max_retries`：删掉 `compact_remote_v2.rs` 那一处。
- 两处都删掉时，删除整个补丁。
- 上游只是给更多场景补上 `Retry-After`：不够，我们用到的服务不一定发，保留本补丁。

## 冲突处理

规则有两条：

1. 容量类错误（`ServerOverloaded`、非配额的 429 兜底 `RetryLimit`）没有服务端建议时，按 `backoff(retry_count)` 重试；有建议时按建议等。
2. 前台远端 V2 压缩不另设重试上限，直接用 provider 的 `stream_max_retries`。

冲突时以上游代码为准，把这两条规则重新套到上游做重试决策和设置压缩上限的位置。上游把这些错误改名、拆分或换了位置时，跟着移过去。用 `rg 'codeks 0008'` 找到 2 处落点。

## 验证

- 不编译的检查：`rg 'codeks 0008' codex-rs` 有 2 处；`rg -n 'handle_response_stream_error\(' codex-rs/core/src --glob '!*tests.rs'` 里，调用方只有 `session/turn.rs`（采样）和 `compact_remote_v2.rs`（前台远端 V2 压缩），多出来的调用方要按「何时扩展」检查；`rg -n 'retry_delay\(' codex-rs --glob '!**/tests/**' --glob '!*_tests.rs'` 里，core 的调用方只有 `responses_retry.rs` 和 `client.rs`，多出来的调用方要按「何时扩展」检查。
- CI（2026-10-16，PR #5 第一轮，测 codex-cloud-config、codex-config、codex-core、codex-protocol）：5842 个测试里 5833 个通过，失败的正是上面 `ci_skip` 里的 9 个；skipped 8 个。第二轮加上 `ci_skip` 后全部通过：5833 个通过，skipped 17 个。去掉远端压缩的上限后：上游涉及远端压缩重试的测试都把 `stream_max_retries` 设成 0、1 或 2，和 2 取最小值、和 `u64::MAX` 取最小值结果相同；没有设置它（默认 5）的压缩测试，注入的都是不重试的错误（`insufficient_quota`、`context_length_exceeded`、400），所以预计不会带来新的失败。去掉上限后又跑了一轮：5833 个通过，skipped 17 个；hooks 和 pending_input_persistence 各有 1 个和本补丁无关的测试第一次失败、重跑通过。
- 用上新版本后：上游返回 503 `server_is_overloaded` 或流内 `server_is_overloaded`、没有 `Retry-After` 时，界面应该显示 “Reconnecting... n/m”，次数用完前不会失败；前台远端 V2 自动压缩时遇到同样的错误，`m` 应该等于 `stream_max_retries`，而不是 2。

## 记录

- 2026-10-16 @ rust-v0.162.1：新增。
- 2026-10-16 @ rust-v0.162.1：远端压缩去掉 2 次重试上限，改用 `stream_max_retries`；补充推荐配置。
- 2026-10-17 @ rust-v0.163.0：同步复核，两处补丁及原有 9 个 `ci_skip` 保留，运行时改动不变。上游 #53800 新增默认关闭的后台压缩，#53451 / #53670 将不完整响应改为终止错误并记录 usage；补充请求边界和监控路径，本轮不扩展重试。

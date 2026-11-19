# 0028 稳定托管搜索发现请求计数测试

```toml
watch_paths = ["codex-rs/app-server/src/request_processors/**", "codex-rs/mcp/src/**"]
watch_regex = ["scoped_lists|tools/list|model_supported_tools"]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

rust-v0.170.0 分组 CI 的 `codex-app-server` 搜索测试中，`model_lab_search_provider::work_hosted_only` 连续两次得到 2 次符合模型元数据条件的 `tools/list`，而上游测试固定要求恰好 1 次；相邻若干参数组合在第一次出现相同差异，但自动重试后通过。没有出现重复的搜索 `tools/call`、搜索结果错误或丢失模型支持工具的元数据。多一次只读发现请求不改变授权和运行结果，因此应验证是否有正确的发现请求，而不是依赖启动及重试时序恰好请求一次。

## 改动

仅修改 `codex-rs/app-server/tests/suite/v2/web_search.rs` 的计数断言。服务端 503 的专门测试继续要求发现请求大于一次；允许托管发现时要求至少一次带正确 `model_supported_tools` 的请求；禁止托管发现时要求零次。原测试对实际搜索工具清单、查询参数、授权策略、缓存回退和真正执行的 `tools/call` 次数的检查全部保留；不修改业务代码和 `ci_skip`。

## 必要性

改变 Web Search 的运行机制来满足一条偶然受重试时序影响的测试计数会影响共享工具发现，远大于测试目标。简单跳过整组测试则会丢失重要的域名/缓存/模型允许列表验证。本修改只消除与业务断言无关的只读发现次数上限。

## 何时扩展

上游变更搜索发现接口、元数据传播或加缓存时，需复查`model_supported_tools` 是否仍传递，禁止托管模式的发现次数是否仍为零；如果新增有副作用的发现操作，不允许仅以次数下限取代授权测试。

## 何时移除

上游提供等价的稳定语义测试或明确规定且实现了 exactly-once 的发现协议时，采用上游更严格的约束并移除本补丁。

## 冲突处理

以上游真实搜索授权/工具调用语义优先；如请求次数代表业务承诺而不只是 MCP 发现重试，应重新检查而不是直接保留本断言。

## 验证

本地执行 rustfmt、git diff --check、补丁栈检查；GitHub Actions 运行完整 `codex-app-server` 分组，检查测试不跳过，包含禁用、503 回退和被托管元数据限定的参数组合。

## 记录

- 2026-11-19 @ rust-v0.170.0：放宽只读、允许的 MCP 搜索发现请求次数为至少一次，保留禁止时零次、错误回退及实际执行次数约束。

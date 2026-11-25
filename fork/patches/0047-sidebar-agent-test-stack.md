# 0047 为侧栏子代理会话测试分配独立栈

```toml
watch_paths = ["codex-rs/core/src/config/mod.rs"]
watch_regex = ["opens_agent_transcript_beside_parent_and_returns_to_chooser|load_config_with_layer_stack"]
backport_of = []
upstream = ["https://github.com/openai/codex/pull/63336"]
ci_skip = []
```

## 背景

同步至 `rust-v0.172.0` 后，Linux CI 的 TUI 调试测试在 8 MiB 的测试线程上运行。子代理会话测试启动内嵌 app-server，加载、恢复并切换多个会话。在用例的两次尝试中，渲染父会话后都因线程栈溢出中止。

## 改动

- `codex-rs/tui/src/app/sidebar_agent_status_tests.rs`：`opens_agent_transcript_beside_parent_and_returns_to_chooser` 使用 16 MiB 测试线程和单线程 Tokio runtime，沿用相邻侧栏文档测试的执行方式。测试名、场景和断言维持原样。

## 必要性

独立线程将更大的栈限制在这个用例。继续运行整个场景可保留对子代理跳转、侧栏关闭和焦点恢复的覆盖。

## 何时扩展

同类真实服务测试如果出现栈溢出，先确认失败发生在哪个线程，再复用对应测试的线程设置。

## 何时移除

上游修复此用例，或缩小相关 future 后，在原有 8 MiB CI 配置下验证此用例通过，即可移除此补丁。

## 冲突处理

保留上游最新测试场景及断言。只有确认默认测试线程仍会栈溢出时才保留独立线程。

## 验证

在 `codeks-ci` 的 `codex-tui` 分组确认该用例实际运行并通过，同时检查测试名和快照断言。

## 记录

- 2026-11-24 @ rust-v0.172.0：为侧栏子代理会话的集成测试分配独立栈。

# 0024 回移：同步增量工具描述快照

```toml
watch_paths = ["codex-rs/core/src/tools/handlers/view_image_spec.rs", "codex-rs/code-mode-protocol/src/description.rs", "codex-rs/features/src/lib.rs"]
watch_regex = ["UnifiedImageBudget|LEGACY_IMAGE_HELPER_DESCRIPTION|UNIFIED_IMAGE_HELPER_DESCRIPTION"]
backport_of = ["e1835142dbf873e17116e1afd9a3a6da79656c26"]
upstream = ["https://github.com/openai/codex/commit/e1835142dbf873e17116e1afd9a3a6da79656c26"]
ci_skip = []
```

## 背景

`rust-v0.167.1` 的普通 Responses 增量工具快照与该基线实际生成的工具描述不一致，导致两个 provider 场景和一个工具移除场景失败。旧快照对应隐藏图像 `detail` 的统一图像预算形状，但基线默认关闭 `unified_image_budget`，实际工具包含 `view_image.detail` 参数、相应输入与输出声明，以及通用 `image()` 助手的 detail 说明。

上游已用 [Align hotfix snapshots with pinned tool descriptions](https://github.com/openai/codex/commit/e1835142dbf873e17116e1afd9a3a6da79656c26) 修正两份快照，但此提交不在固定基线中。直接回移该修复，避免等待下一次基线升级才能恢复这些测试。

## 改动

原样应用上游提交的两文件差异，保留上游哈希和断言行号元数据，不修改运行时行为或跳过测试：

- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__incremental_tools__incremental_tools_regular_responses.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__incremental_tools__incremental_tool_removals_regular_responses.snap`

## 必要性

问题是上游快照陈旧，不是增量工具协议或本 fork 的图片环境字段发生变化。改配置去迎合旧快照会改变产品行为，改运行时或跳过测试也会掩盖真正的测试预期。用户已选择原样回移已合入的上游修复；独立补丁便于升级时完整删除。

## 何时扩展

不把其他快照更新并入本回移。若工具描述、`UnifiedImageBudget` 默认值或图像助手说明继续变化，先比对具体 JSON 字段和新的上游修复，再判断是否已由基线覆盖。

## 何时移除

新基线包含 `e1835142dbf873e17116e1afd9a3a6da79656c26`、其原始提交 `dfed4e6853549a377f58fe8441a719f0f675f5d2`，或两份快照的等价修复时，删除整个 0024 补丁。无需等待任何 executor Skill、MCP 或图片环境功能合入上游。

## 冲突处理

以新基线的实际工具描述和对应上游快照为准，不强行保留这里的旧哈希或断言行号。若上游已修复，直接删除补丁；若仍需回移，只应用上述上游提交，不手工接受未知来源的新哈希。

## 验证

本地比较两文件与上游提交的 blob，并运行 `git diff --check`。未在本地编译或执行 Rust 测试；远端 CI 应运行原有三个失败场景。

已用发行构建和隔离模型 fixture 捕获实际工具 JSON，逐字段逆变换重现所有旧 namespace 哈希：移除 `view_image.detail` 即复现普通工具旧值；Code Mode 还需移除其输入、输出 detail 声明并切换图像助手说明；压缩后场景同样由 detail 声明解释。上游新快照与远端 CI 的实际哈希一致，因此此次更新有具体字段依据。

## 记录

- 2026-11-11 @ rust-v0.167.1：原样回移上游增量工具快照修复，保留运行时行为与全部测试。

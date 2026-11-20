# 0031 刷新核心模型上下文快照

```toml
watch_paths = ["codex-rs/core/tests/suite/snapshots/**", "codex-rs/prompts/src/**"]
watch_regex = ["format_request_history_snapshot|portable_tool_schema|model_messages|Decision Boundary"]
backport_of = []
upstream = ["https://github.com/openai/codex/pull/62124"]
ci_skip = []
```

## 背景

同步至 rust-v0.170.0 后，部分 Core 场景快照仍记录旧版工具定义指纹、插件提示和测试宏元数据。真实 CI 用 `INSTA_UPDATE=always` 生成候选并导出 ZIP，逐文件比较后确认：63 份不同快照中，33 份**仅更新 hash 指纹**；另外 30 份的全部非指纹差异可归入 9 类确定变化，只有新版通用插件 `Decision Boundary` 提示词及省略长度，或 Insta 的 `assertion_line` / `expression` 元数据。没有改变模型请求中的工具名称/数量、权限、消息历史或结果。已有其他上游场景快照也使用同样新版提示词。

## 改动

仅更新以下 63 个上游测试快照，不修改运行时代码、测试执行逻辑或 `ci_skip`。这些期望来自 rust-v0.170.0 真实 Core 测试生成结果；完整路径清单用于补丁栈校验及未来按文件删除已被上游覆盖的条目：

- `codex-rs/core/tests/suite/snapshots/all__suite__request_plugin_install__catalog_plugin_recommendations_absent.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__request_plugin_install__catalog_plugin_recommendations_blank.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__request_plugin_install__catalog_plugin_recommendations_custom.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__request_plugin_install__catalog_plugin_recommendations_disabled.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__request_plugin_install__catalog_plugin_recommendations_empty.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__request_plugin_install__catalog_plugin_recommendations_null.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__request_plugin_install__catalog_plugin_recommendations_oversized.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__agent_eviction__idle_eviction_followup.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__agent_message_board__remote__remote_board_permissions_direct.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__astra_active_environment_selection.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__astra_async_compaction_with_live_suffix.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__astra_async_question_and_answer.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__astra_code_mode_call_timing.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__astra_disabled_executor_skills.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__astra_executor_plugin_policy_refresh.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__astra_input_interrupts_response.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__astra_input_yields_code_mode_cell.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__astra_kickoff_remote_compaction_windows.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__astra_plugin_policy_limits_visible_skills.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__astra_plugin_policy_without_skill_descriptions.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__astra_plugin_refresh_simplified_selection_hints_off.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__astra_plugin_refresh_simplified_selection_hints_on.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__astra_settings_release_check_tool_shapes.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__attached_side_chat_preserves_model_context_on_cold_resume.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__budget_context__host_budget_context_after_compaction.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__cached_stdio_stays_dormant_in_a_new_chat_until_first_use.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__catalog_selected_plugin_wording_apps_false_simplified_false.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__catalog_selected_plugin_wording_apps_true_simplified_false.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__catalog_selected_plugin_wording_apps_true_simplified_true.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__child_completion_wakes_parent_after_final_answer.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__code_mode_catalog_messages.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__code_mode_catalog_messages_ranked_search.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__code_mode_mcp_schema_limits.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__code_mode_settled_helpers__code_mode_settled_helpers.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__command_sessions__command_sessions_world_state.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__compaction__remote_slash_compact.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__dynamic_tools__dynamic_tools_update_preserves_pending_calls_and_survives_resume.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__frozen_cloud_skills__frozen_cloud_skills_with_named_executor_request_history.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__incremental_tools__incremental_tool_removals.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__incremental_tools__incremental_tool_removals_regular_responses.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__incremental_tools__incremental_tools_catalog_messages.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__incremental_tools__prediction_fork_inherited_tools.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__indirect_namespace_prefixes__catalog_indirect_namespace_prefixes.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__mcp_ema__enterprise_failure_preserves_ordinary_mcp_history.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__mcp_ema__enterprise_revocation_preserves_history_and_ordinary_tools.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__mcp_resource_messages__mcp_resource_messages.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__mcp_sharing__shared_mcp_keeps_tools_and_model_requested_input_per_chat.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__mcp_tool_discovery_CodeModeOnly.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__mcp_tool_discovery_Direct.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__multi_agent_catalog_parameters.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__multi_environment_mcp__same_named_executor_mcps_keep_calls_and_approvals_separate.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__partial_answers__preserved_answer_fork.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__prediction_fork_preserves_context_above_compaction_threshold.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__retained_tool_outputs__retained_tool_output_retention_disabled.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__retained_tool_outputs__retained_tool_output_survives_compaction_and_resume.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__skill_catalog_dedup__dedup_before_budgeting.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__skill_catalog_dedup__dedup_without_descriptions.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__skill_catalog_dedup__skill_catalog_deltas.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__skill_read_installation_recovery_request_history.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__strict_3p_cache__strict_3p_mcp_catalog_cache.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__unavailable_images_are_rechecked_without_rewriting_history.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__world_state_prefix__extension_window_prefix_local.snap`
- `codex-rs/core/tests/suite/snapshots/all__suite__scenarios__world_state_prefix__extension_window_prefix_remote.snap`

## 必要性

这些回归快照验证模型工具授权、子代理、插件目录、增量工具和消息历史。直接跳过测试或把运行时工具声明回退到旧哈希都会掩盖真正的回归。这里只更新经核对的稳定期望，不能把未来任何 Insta 候选一概当作正确结果。

## 何时扩展

上游新增或调整开发者指令、工具目录、schema 或上下文快照渲染/哈希时，逐项核对新增差异，尤其是工具暴露范围、权限与消息行为。不得在正式 CI 中自动接受未审查快照。

## 何时移除

当新的上游基线已经包含等价修正（如后续 #62124），逐文件删除被覆盖的条目；全部由上游覆盖时删除整个补丁。

## 冲突处理

新基线优先，依据实际测试结果和授权语义裁决；如果上游重写了场景，不用旧快照覆盖新增断言。

## 验证

GitHub Actions 运行完整 `codex-core` 真实测试并导出候选 ZIP；本地校验 63 个 ZIP 路径、每份旧新差异和 33/30 类别。通过 `git diff --check` 和 `codeks.py check`；正式 CI 关闭 `INSTA_UPDATE=always`、不新增 `ci_skip`，必须让所有核心快照真实通过。网络审批等独立失败需另行处理。

## 记录

- 2026-11-19 @ rust-v0.170.0：逐份审查后更新 63 份 Core 场景快照。

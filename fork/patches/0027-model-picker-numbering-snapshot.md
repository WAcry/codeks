# 0027 更新模型选择器编号快照

```toml
watch_paths = [
  "codex-rs/tui/src/bottom_pane/list_selection_view.rs",
  "codex-rs/tui/src/chatwidget/tests/model_picker_tests.rs",
  "codex-rs/tui/src/chatwidget/tests/popups_and_settings.rs",
]
watch_regex = ["auto_number_shortcuts|enabled_row_number|model_selection_popup"]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

同步到 rust-v0.170.0 后，TUI 的五个模型选择器测试都因同一份快照不匹配而失败；完整 CI 分组验证确认其余 TUI 测试通过。该快照仍按旧版显示方式省略第 10、11 项的编号，而当前上游 `ListSelectionView` 使用 `auto_number_shortcuts` 默认渲染所有启用行的序号，第 10、11 项分别显示为 `10.`、`11.`。该问题在未改动模型选择器实现的上游基线也存在，不是 fork 修改了模型路由或选择器行为。

## 改动

仅更新 `codex-rs/tui/src/chatwidget/snapshots/codex_tui__chatwidget__tests__model_selection_popup.snap` 的两行：显示第 10、11 项的实际编号与选中光标缩进。保留模型顺序、描述、快捷键事件和全部测试断言；不更改运行时代码，也不新增 `ci_skip`。

## 必要性

该快照被五个独立测试复用，错误的固定预期会让整个 TUI 测试组失败，并掩盖实际回归。配置或代理层不能改变这个测试夹具；更新共享快照的两行比更改上游模型选择器、删除测试或跳过测试更小，也不会改变用户行为。既定原则是遵循新版上游的实际行为，而不是为满足旧快照回退新代码。

## 何时扩展

上游调整 `ListSelectionView` 的序号、数字快捷键或模型目录项数时，重新执行 TUI 模型选择器测试，核实这份快照是否仍匹配实际渲染；不从单份快照推断全部选择器的快捷键行为。

## 何时移除

上游修正了等价快照并进入新基线、或模型选择器改用其他可验证的稳定断言时，删除此补丁，直接采用上游结果。

## 冲突处理

以上游的选择器行为及最新测试为准；若上游已修改该快照，先核对实际渲染和模型顺序，相同内容直接丢弃此补丁，不能覆盖更全面的新上游修改。

## 验证

本地检查 `git diff --check`、两行快照和 `python3 fork/tools/codeks.py check`。GitHub Actions `codeks-ci` 的 TUI 分组应运行全部模型选择器测试；此前失败的五个案例必须实际通过，不用 `ci_skip` 把它们藏起来。

## 记录

- 2026-11-19 @ rust-v0.170.0：修正上游未随实际编号渲染更新的模型选择器共享快照，保持运行时行为不变。

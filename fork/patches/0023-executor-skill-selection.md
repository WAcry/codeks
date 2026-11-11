# 0023 保持执行环境 Skill 的原生显式加载语义

```toml
watch_paths = ["codex-rs/core/src/session/**", "codex-rs/skills/src/injection.rs"]
watch_regex = ["SkillInstructions|collect_explicit_skill_mentions|truncate_main_prompt_contents|execution_context"]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

executor Skill 已走原生 Mention、正文读取和历史路径，但普通用户 Skill 的正文被额外截断，片段缺少执行环境信息。若显式选择的电脑 Skill 缺失，消息内同名 `$name` 还可能选择 Host Skill。

## 改动

普通 executor 用户 Skill 与普通 Host 用户 Skill 一样保留全文；插件和云端的原有截断策略及底层文件读取限制不变。原生 SkillInstructions 增加独立执行环境和 Skill 根目录信息。原生 `skill://` Mention 的名称阻止同名文本回退，即使选中项不可用也不换用 Host 同名 Skill。

继续使用同一个原生片段、同回合去重、历史记录和压缩流程，不新增激活表、客户端正文拼接或另一种消息。该补丁不依赖 0019 目录 API 或 0022 用户开关。

涉及路径：

- `codex-rs/ext/skills/src/extension.rs`
- `codex-rs/ext/skills/src/fragments.rs`
- `codex-rs/ext/skills/src/host_prompt.rs`
- `codex-rs/ext/skills/tests/skills_extension.rs`
- `codex-rs/ext/skills/tests/skills_extension/codeks_executor_selection_tests.rs`
- `codex-rs/skills/src/selection.rs`
- `codex-rs/skills/src/codeks_skill_selection_tests.rs`

## 必要性

配置或客户端注入正文无法修正原生选择与历史语义，也无法保证电脑脚本继续在正确环境执行。用户要求电脑 Skill 与 Host 一样由原生选择流程加载，因而在已有准备回合和选择器边界做窄修改。

## 何时扩展

上游改变独立 Skill 与插件的正文策略、结构化 Mention 分类或 SkillInstructions 时，重新确认环境来源、全文和同名选择规则仍一致。不要扩展成跨回合自动去重或长期激活状态。

## 何时移除

上游普通 executor 用户 Skill 不再有独立的额外正文截断时，删除 extension 中对应条件；上游原生片段提供执行环境与根目录时，删除 execution_context 字段和构造；上游原生 `skill://` Mention 已阻止不可用选择回退到同名 Host 时，删除 selection 的名称阻止块。三处可按对应上游能力逐块缩小；全部满足后删除 0023，不牵动目录 API 和用户开关。

## 冲突处理

保留上游片段类型、内容元数据、历史和压缩实现，不替换原生注入路径。全文条件只覆盖独立 executor Skill；Host 片段继续不附加 executor 来源。只拦截 `skill://`，保留 Host 原生路径 Mention 配合 `$name` 的既有行为，不新增 Host 路径解析；不要禁止其他无关 Skill 的文本选择。

## 验证

未运行单元测试。本地做相关文件 rustfmt、源码检查与 `git diff --check`；协议产物由原生 schema writer 生成，只执行生成所需的编译和 writer，不执行单元测试套件。测试执行交给 CI。

测试覆盖重复 Mention 与 `$name` 同回合仅一个原生片段、来源信息、超过八千字节的完整正文，以及不可用 selector 只阻止 Host 同名 Skill；Host 原生路径 Mention 配合 `$name` 继续选择该 Host Skill。发行包上检查原始模型请求包含独立 Skill 片段，恢复历史无需为了展示再次读取电脑文件。

## 记录

- 2026-11-11 @ rust-v0.167.1：独立修正 executor 显式 Skill 的正文、来源和同名回退语义。

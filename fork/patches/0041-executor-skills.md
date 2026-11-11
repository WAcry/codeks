# 0041 执行环境原生 Skill 目录、开关与加载

```toml
watch_paths = [
  "codex-rs/core/src/session/**",
  "codex-rs/core/src/tasks/**",
  "codex-rs/exec-server/src/discoverV2/**",
  "codex-rs/skills/src/injection.rs",
]
watch_regex = [
  "disabled_skill_paths|disabledSkillPaths|SkillConfigRules|skills.config",
  "skills/catalog|SkillsCatalog|SkillListQuery|list_from_discovery_v2",
  "SkillInstructions|collect_explicit_skill_mentions|truncate_main_prompt_contents|execution_context",
]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

`skills/list` 只列控制端目录，客户端要为指定电脑环境展示正确的 Skill 菜单，也要能查询菜单而不恢复线程或启动 Goal。executor V2 发现独立 Skill 后缺少用户启用规则的投影。显式选择电脑 Skill 时，原生正文还可能被额外截断，来源信息不全，或在文件不可用时回退到 Host 的同名 Skill。

## 改动

### 目录查询

`skills/catalog` 接受显式 `environments`、cwd 和可选 `forceReload`。每个环境返回原生 selector、目标机器的 `SKILL.md` 路径、可见性和启用状态。环境错误单独返回，远端查询最多等待五秒。Host 使用原生加载服务，executor 使用 V2 provider 的独立用户及系统 Skill 投影。该接口无需创建或恢复线程，`skills/list`、线程插件选择和运行时权限继续由原生流程处理。

- 协议及 API：`codex-rs/app-server-protocol/src/protocol/v2/skill_catalog.rs`、`codex-rs/app-server-protocol/src/protocol/v2/mod.rs`、`codex-rs/app-server-protocol/src/protocol/common.rs`、`codex-rs/app-server/src/message_processor.rs`、`codex-rs/app-server/src/request_processors/catalog_processor.rs`、`codex-rs/app-server/src/request_processors/catalog_processor/executor_skill_catalog.rs`。
- 目录投影：`codex-rs/ext/skills/src/catalog.rs`、`codex-rs/ext/skills/src/provider.rs`、`codex-rs/ext/skills/src/provider/host.rs`、`codex-rs/ext/skills/src/provider/executor/discovery_v2.rs`。
- API 用例：`codex-rs/app-server/tests/suite/v2/mod.rs`、`codex-rs/app-server/tests/suite/v2/codeks_executor_skill_catalog.rs`。
- 生成的 JSON：`codex-rs/app-server-protocol/schema/json/ClientRequest.json`、`codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.schemas.json`、`codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.v2.schemas.json`、`codex-rs/app-server-protocol/schema/json/v2/SkillsCatalogParams.json`、`codex-rs/app-server-protocol/schema/json/v2/SkillsCatalogResponse.json`。
- 生成的 TypeScript：`codex-rs/app-server-protocol/schema/typescript/ClientRequest.ts`、`codex-rs/app-server-protocol/schema/typescript/v2/SkillCatalogMetadata.ts`、`codex-rs/app-server-protocol/schema/typescript/v2/SkillsCatalogEntry.ts`、`codex-rs/app-server-protocol/schema/typescript/v2/SkillsCatalogParams.ts`、`codex-rs/app-server-protocol/schema/typescript/v2/SkillsCatalogResponse.ts`、`codex-rs/app-server-protocol/schema/typescript/v2/index.ts`。
- 预计算导出：`codex-rs/app-server-protocol/schema/precomputed/app-server-exports-experimental.json.zst`、`codex-rs/app-server-protocol/schema/precomputed/app-server-exports-stable.json.zst`。

### executor 用户开关

每次 V2 响应读取 executor 自己的 `CODEX_HOME/config.toml`，复用 `SkillConfigRules`，在 executor 上规范化路径和软链接，后匹配规则覆盖先匹配规则。缺少文件等同空规则，读取或解析错误向调用方返回。更新随下一次发现和下一任务生效，原有目录缓存和插件策略仍适用。

可选 `disabledSkillPaths` 将禁用的独立 Skill 路径投影给目录与运行时。新 executor 的空数组表示支持开关且无禁用项；旧响应缺少字段时仍可读取目录。

- 读取、传输与测试：`codex-rs/config/src/skills_config.rs`、`codex-rs/exec-server-protocol/src/capabilities.rs`、`codex-rs/exec-server-protocol/src/capabilities_tests.rs`、`codex-rs/exec-server/src/lib.rs`、`codex-rs/exec-server/src/server/handler.rs`、`codex-rs/exec-server/src/executor_skill_config.rs`、`codex-rs/exec-server/src/executor_skill_config_tests.rs`。
- 扩展用例：`codex-rs/ext/skills/tests/skills_extension.rs`、`codex-rs/ext/skills/tests/skills_extension/codeks_executor_catalog_tests.rs`。

### 显式选择与正文

executor 的原生 `skill://` selector 作为 `UserInput::Mention.path` 传入。选中的 Skill 不可用时，同名文本标记不能改选 Host Skill。Host 的原生 `UserInput::Skill.path` 及路径 Mention 与 `$name` 的现有配合继续适用。普通 executor 用户 Skill 使用原生读入上限以内的全文，附执行环境和根目录，继续使用原生片段、同回合去重、历史及压缩。插件、云端及文件读取上限继续沿用原策略，Host 片段保持原格式。

- 实现：`codex-rs/ext/skills/src/extension.rs`、`codex-rs/ext/skills/src/fragments.rs`、`codex-rs/ext/skills/src/host_prompt.rs`、`codex-rs/skills/src/selection.rs`。
- 回归用例：`codex-rs/core/tests/suite/skills_extension.rs`、`codex-rs/ext/skills/tests/skills_extension/codeks_executor_selection_tests.rs`、`codex-rs/skills/src/codeks_skill_selection_tests.rs`。

## 必要性

原生 executor provider 同时服务菜单、自动选择和显式加载。将三个环节放在同一补丁可一起审查 selector、禁用状态与实际执行一致性。客户端扫文件、另存禁用清单或拼接 Skill 正文会绕开原生环境解析与历史路径。现有实现复用加载器和投影，在协议和准备回合的挂载点补齐能力。

## 何时扩展

上游修改 V2 发现、缓存、用户配置规则、目录可见性、资源 URI 或原生 SkillInstructions 时，复核目录与运行时的一致性。若将目录扩展到 executor 插件，先接入原生插件接受状态。显式选择继续限定单回合，不引入长期激活状态。

## 何时移除

上游支持按环境、cwd 查询原生目录，独立返回环境错误，且无需创建或恢复线程时，可迁移目录 API。用户规则在 executor 内生效并能表达旧响应与空状态时，可改用上游投影。正文、来源及同名回退各自有等价实现后，可逐处删除对应改动。

## 冲突处理

保留上游原生 Skill 发现、权限、回合注入、历史和压缩流程。把目录查询留在适配模块，冲突时逐项保留后规则优先、缺失字段兼容、独立 Skill 范围和 `skill://` 精确匹配。用上游 schema writer 同步生成 JSON、TypeScript 和预计算导出。

## 验证

- 目录用例覆盖 Host、Windows executor、旧响应、单环境错误及不创建线程。用户规则用例覆盖覆盖顺序、配置更新与目录缓存、解析错误、软链接和多环境独立状态。
- 显式选择用例覆盖同回合去重、未选择时不重新注入、再次选择时重新读取、来源、超过八千字节的完整正文、1 MiB 上限和同名回退行为。跨回合测试与历史恢复分别验证。
- 发行包上从目录获得 selector，用于发送原生 Mention；关闭后下一任务不自动或显式加载，重新启用后恢复。核对模型请求中的来源和原生片段，以及已保存历史的显示。

## 记录

- 2026-11-11 @ rust-v0.167.1：新增按环境目录、用户开关投影及原生加载修正。
- 2026-11-18 @ rust-v0.170.0：更新 V2 provider 构造、测试 fixture 与原生协议生成产物。
- 2026-11-20 @ rust-v0.171.0：合并原 0019、0022、0023。

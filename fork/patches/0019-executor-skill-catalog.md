# 0019 按执行环境提供原生 Skill 目录

```toml
watch_paths = ["codex-rs/core/src/session/**", "codex-rs/exec-server/src/discoverV2/**"]
watch_regex = ["skills/catalog|SkillsCatalog|SkillListQuery|list_from_discovery_v2"]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

`skills/list` 只列出控制端目录，客户端无法为电脑环境提供准确的 Skill 选择菜单。读取菜单不能创建或恢复 Thread，因为恢复可能启动 Goal。

## 改动

增加 `skills/catalog`，请求直接携带 `environments` 和可选 `forceReload`，不接受 Thread ID。每个环境返回原生 selector、目标机器的 `SKILL.md` 路径、可见性和启用状态。错误按环境返回，executor 查询最多等待五秒。Host 复用原生加载服务和目录投影；executor 复用原生 V2 provider 投影，只列出独立用户/系统 Skill，不介入 Thread 的插件接受状态。

这是目录检查，不是任意 Thread 的权限快照；执行仍由原生准备回合时的 sandbox 和授权决定。现有 `skills/list` 不变。executor 的 `skill://` selector 原样发送为 `UserInput::Mention.path`；Host 的原生路径保留现有 `UserInput::Skill.path` 输入方式，不承诺新增 Host 路径 Mention 的独立解析语义。本补丁依赖 0022 提供可选禁用路径和支持状态，API 只读取它，不实现规则。0023 的显式正文加载不属于本补丁。

涉及路径：

- `codex-rs/app-server-protocol/src/protocol/v2/skill_catalog.rs`
- `codex-rs/app-server-protocol/src/protocol/v2/mod.rs`
- `codex-rs/app-server-protocol/src/protocol/common.rs`
- `codex-rs/app-server/src/message_processor.rs`
- `codex-rs/app-server/src/request_processors/catalog_processor.rs`
- `codex-rs/app-server/src/request_processors/catalog_processor/executor_skill_catalog.rs`
- `codex-rs/app-server/tests/suite/v2/mod.rs`
- `codex-rs/app-server/tests/suite/v2/codeks_executor_skill_catalog.rs`
- `codex-rs/ext/skills/src/catalog.rs`
- `codex-rs/ext/skills/src/provider.rs`
- `codex-rs/ext/skills/src/provider/host.rs`
- `codex-rs/ext/skills/src/provider/executor/discovery_v2.rs`


协议生成产物由上游 schema writer 从 Rust 类型导出：

- `codex-rs/app-server-protocol/schema/json/ClientRequest.json`。
- `codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.schemas.json`。
- `codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.v2.schemas.json`。
- `codex-rs/app-server-protocol/schema/json/v2/SkillsCatalogParams.json`。
- `codex-rs/app-server-protocol/schema/json/v2/SkillsCatalogResponse.json`。
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-experimental.json.zst`。
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-stable.json.zst`。
- `codex-rs/app-server-protocol/schema/typescript/ClientRequest.ts`。
- `codex-rs/app-server-protocol/schema/typescript/v2/SkillCatalogMetadata.ts`。
- `codex-rs/app-server-protocol/schema/typescript/v2/SkillsCatalogEntry.ts`。
- `codex-rs/app-server-protocol/schema/typescript/v2/SkillsCatalogParams.ts`。
- `codex-rs/app-server-protocol/schema/typescript/v2/SkillsCatalogResponse.ts`。
- `codex-rs/app-server-protocol/schema/typescript/v2/index.ts`。

## 必要性

客户端扫描文件或自行构造 selector 会与原生运行时分叉。新模块只适配已有加载器和投影，不新增目录引擎或会话生命周期。用户选择原生目录 API；检查当前可用上游历史，尚无可直接回移的等价接口。

## 何时扩展

上游改变 V2 provider、目录可见性或资源 URI 时，校准投影和响应。若要列出 executor 插件，必须先接入原生插件接受状态，不能直接去掉过滤条件。

## 何时移除

上游接口同时满足以下条件后，迁移调用方并整块删除本补丁：无需创建/恢复 Thread；接受显式环境和 cwd；返回原生 selector、目标路径、启用和显式调用策略；离线环境不阻止其他目录返回。删除时保留独立的 0022 用户规则及 0023 加载语义。若上游用其他字段表达启用支持，先更新本接口适配，再移除 0022，不能继续依赖已删除字段。

## 冲突处理

目录逻辑保留在新模块，热文件只留注册和调用。`provider/executor/discovery_v2.rs` 只修改方法可见性的块属于 0019，禁用投影属于 0022。同步生成所有协议产物，不手工猜测 schema。

## 验证

未运行单元测试。本地做相关文件 rustfmt、源码检查与 `git diff --check`；协议产物由原生 schema writer 生成，只执行生成所需的编译和 writer，不执行单元测试套件。测试执行交给 CI。

API 测试覆盖 Host 与 Windows executor 路径、旧 executor、单环境错误以及查询前后没有 Thread。发行包上确认目录 selector 可直接作为 Mention，隐藏 Skill 仍可显式选择；不通过恢复 Thread 获得菜单。

## 记录

- 2026-11-11 @ rust-v0.167.1：新增无 Thread 副作用的环境目录，用户规则与加载语义分别归属 0022 和 0023。

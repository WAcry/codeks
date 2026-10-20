# 0009 默认关闭上游更新检测

```toml
watch_paths = ["codex-rs/tui/src/updates.rs", "codex-rs/cli/src/doctor/updates.rs"]
watch_regex = ["check_for_update_on_startup", "get_upgrade_version"]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

上游默认在 TUI 启动时检测更新，并展示升级提示。检测和升级目标都是官方 Codex，无法表示 codeks 的补丁版本；提示中的命令还可能把 fork 换回官方包。用户希望 codeks 开箱即关闭这项检测，而不必在每处安装补写配置。

用户接受保留显式配置开启的能力；沿用现有开关比删除整个检测机制更小、更容易同步。

## 改动

- `codex-rs/core/src/config/mod.rs`：把 `check_for_update_on_startup` 缺省值由 `true` 改为 `false`，同步字段注释。显式 `true` / `false` 仍按现有配置加载规则生效。
- `codex-rs/config/src/config_toml.rs`：同步配置字段注释。
- `codex-rs/core/config.schema.json`：同步同一字段的生成描述，不新增 schema 默认值。

现有 TUI 的 `get_upgrade_version` 和 `get_upgrade_version_for_popup` 都先检查这个开关，因此默认不发起后台版本查询，也不显示缓存里的升级提示。用户主动运行 `codex doctor` 的版本诊断、手动升级以及其他通知不变。

## 必要性

推荐 `check_for_update_on_startup = false` 能获得相同行为，但需要用户维护每处配置，不能改变未配置时的默认体验。按用户本次选择，使用一行运行时补丁统一默认值，保留显式开启。

bridge 只处理模型请求，无法控制客户端启动检测。新增模块或改写 TUI 会比修改现有配置默认值增加更多挂载点。配置文件是上游热点，但本补丁只改一个默认值及对应说明，不分叉更新业务逻辑。

## 何时扩展

- 默认值解析迁移到其他模块：跟随唯一默认值来源，不在多个调用方重复设值。
- 上游新增不受此开关控制的自动启动检测或提示：复核是否属于本补丁覆盖的行为，再决定补充适配。
- 更新目标变为可配置：与用户比较改用配置、指向 fork 发布或继续默认关闭的成本。

## 何时移除

- 上游自身默认关闭启动更新检测，或移除该机制。
- 用户改为统一部署配置，并决定以配置替代这个默认值补丁。

## 冲突处理

保留上游加载和约束处理，只将未配置时的默认值设为 `false`。核对显式 `true` 仍可开启；同步注释和 schema 描述，不移动周边配置逻辑。

## 验证

- 本地运行 `codeks.py check`，静态核对唯一运行时默认值和两个 TUI 入口；不为此本地编译。
- schema 描述应与 `ConfigToml` 文档注释相同；现有远端 `config_schema_matches_fixture` 测试负责生成结果的一致性。
- 使用发布包时，未配置或显式 `false` 应关闭启动查询和提示，显式 `true` 恢复上游行为。未配置时，即使存在旧的版本缓存，也不展示更新提示。

## 记录

- 2026-10-20 @ rust-v0.163.0：新增。

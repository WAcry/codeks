# 0022 应用执行环境的用户 Skill 开关

```toml
watch_paths = ["codex-rs/exec-server/src/discoverV2/**", "codex-rs/core/src/tasks/**"]
watch_regex = ["disabled_skill_paths|disabledSkillPaths|SkillConfigRules|skills.config"]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

executor V2 已发现用户 Skill，却没有应用自己 `CODEX_HOME/config.toml` 中的开关。只在客户端隐藏菜单无法禁止运行时使用。

## 改动

每次 V2 响应前读取 executor 自身 `[skills]`，复用原生 `SkillConfigRules`，把独立用户/系统 Skill 的禁用路径返回给控制端。name/path 规则保持原生顺序，后匹配规则覆盖先匹配规则。路径和软链接在 executor 上解析；不复制、移动或隐藏 Skill。目录缓存及插件策略不变，配置更新随下一次发现和下一任务生效。缺失配置等同空规则，读取和解析错误直接返回。

响应新增可选 `disabledSkillPaths`：新 executor 返回数组，空数组表示支持但没有禁用项；旧 executor 缺失字段仍可使用目录。运行时 provider 应用这一投影。0022 不增加目录 RPC，也不改变显式正文加载；0022 可以先于 0019 独立应用。

涉及路径：

- `codex-rs/config/src/skills_config.rs`
- `codex-rs/exec-server-protocol/src/capabilities.rs`
- `codex-rs/exec-server-protocol/src/capabilities_tests.rs`
- `codex-rs/exec-server/src/lib.rs`
- `codex-rs/exec-server/src/server/handler.rs`
- `codex-rs/exec-server/src/executor_skill_config.rs`
- `codex-rs/exec-server/src/executor_skill_config_tests.rs`
- `codex-rs/ext/skills/src/provider/executor/discovery_v2.rs`
- `codex-rs/ext/skills/tests/skills_extension.rs`
- `codex-rs/ext/skills/tests/skills_extension/codeks_executor_catalog_tests.rs`

## 必要性

仅写配置不能补齐原生缺失的读取逻辑；客户端另建禁用表又会漏掉模型自动选择。把规则解释留在 executor，复用原生优先级，是用户确认的最小实现。standalone executor 需要升级；控制端按可选字段说明支持能力，无需版本状态机。

## 何时扩展

上游改变 SkillConfigRules、V2 缓存或任务刷新边界时，检查是否仍按最新用户配置投影。若增加项目配置层，应复用上游已定义的环境配置机制，不自行复制控制端完整配置加载器。

## 何时移除

上游 V2 已在 executor 上应用用户级 name/path 规则，且修改后下一任务生效、结果可供控制端识别启用与支持状态时，删除本补丁的读取模块和 provider 过滤。先把 0019 的支持字段适配到上游响应，再删除本字段。仅上游支持 Host 开关不满足删除条件；无需等上游提供 Composer API 或改正文加载。

## 冲突处理

保留上游规则顺序和路径规范化。`provider/executor/discovery_v2.rs` 的禁用投影属于 0022，公开方法属于 0019；测试通过原有 SkillProvider::list，不依赖 0019。`disabledSkillPaths` 缺失与空数组必须保持区别，插件不套用独立 Skill 开关。

## 验证

未运行单元测试。本地做相关文件 rustfmt、源码检查与 `git diff --check`；协议产物由原生 schema writer 生成，只执行生成所需的编译和 writer，不执行单元测试套件。测试执行交给 CI。

测试覆盖后规则覆盖前规则、配置更新但目录缓存不变、旧响应、解析错误、软链接及多环境独立状态。发行包上关闭一个电脑 Skill，确认下一任务不会自动或显式加载；再开启后恢复，文件内容和路径不变。

## 记录

- 2026-11-11 @ rust-v0.167.1：独立提供 executor 用户 Skill 开关投影。

# 0013 保留已有线程的模型提供方

```toml
watch_paths = [
  "codex-rs/app-server/src/request_processors/thread_processor.rs",
  "codex-rs/app-server/src/request_processors/turn_processor.rs",
  "codex-rs/app-server/src/request_processors/thread_goal_processor.rs",
  "codex-rs/app-server/src/request_processors/thread_queue_processor.rs",
  "codex-rs/app-server/src/config_manager_provider_tests.rs",
  "codex-rs/app-server/tests/suite/v2/model_provider_enforcement_tests.rs",
  "codex-rs/app-server/tests/suite/v2/thread_resume.rs",
  "codex-rs/app-server/tests/suite/v2/mcp_resource.rs",
]
watch_regex = ["check_thread_model_provider_for_input|load_for_resume|ModelProviderMismatch|required_model_provider"]
backport_of = []
upstream = ["https://github.com/openai/codex/pull/57840"]
ci_skip = [
  "package(codex-app-server) & test(=config_manager::provider_tests::input_provider_check_uses_effective_selection::explicit_provider)",
  "package(codex-app-server) & test(=config_manager::provider_tests::input_provider_check_uses_effective_selection::selected_profile)",
  "package(codex-app-server) & test(=suite::v2::model_provider_enforcement::provider_requirement_changes_reject_inputs_to_existing_threads::default_selection_changes)",
  "package(codex-app-server) & test(=suite::v2::model_provider_enforcement::provider_selection_can_be_restored_for_existing_threads)",
  "package(codex-app-server) & test(=suite::v2::thread_resume::thread_resume_recovers_after_provider_configuration_is_restored::provider_mismatch)",
]
```

## 背景

上游 0.166.0 的 PR #57840 要求已有线程的 provider 和当前默认选择一致。默认从 A 改成 B 后，恢复或向 A 的旧线程输入都会先返回 `modelProviderMismatch`；即使客户端明确传回保存的 A，也不能继续。

用户选择保留每个线程自己的 provider：默认 B 用于新线程，旧线程继续使用 A，客户端保存的 model 和 effort 也继续按线程恢复。用户已明确选择在 Codeks 修改原生准入逻辑，不采用将旧线程改绑到 B 的方案。

## 改动

只解除当前默认选择与线程 provider 必须相等的限制，沿用已有恢复、输入和持久化实现。

- `codex-rs/app-server/src/config_manager.rs`：输入校验继续检查当前管理政策并加载当前配置，但不比较其默认 provider 与已有 session 的 provider。冷恢复保留前置配置加载、错误传播位置，以及 saved/requested provider 的存在性和政策校验，仅删除与当前默认 provider 的相等性判断。
- `codex-rs/app-server/tests/suite/v2/mod.rs`：注册独立回归模块。
- `codex-rs/app-server/tests/suite/v2/thread_provider_defaults.rs`：使用两个独立的模型服务，验证默认 A 改 B 后热恢复和输入仍到 A、新线程到 B；验证重启后按客户端保存的 provider/model/effort 冷恢复同一线程，再显式 fork 到 A，实际请求仍保留设置和历史。

保留配置加载的次数、顺序和错误优先级，避免改变恢复过程中宿主配置回调、并发元数据变更及重新加载的边界。既有 `mcp_resource` 回归中的两次加载和重试语义不变。当前配置无法加载时仍拒绝输入，不因本补丁绕过原有校验。

已有线程的管理政策改变时仍可被拒绝；冷恢复缺少 provider 定义、或保存/请求的 provider 与显式政策冲突时，仍返回原生结构化错误。保留 `ModelProviderMismatch` 协议类型，不做 schema 变更。这里不新增活跃线程 provider 存在性预检，也不冻结 endpoint 或凭据配置、不改写历史、不新增迁移或重绑机制。

元数据中只排除 5 个直接断言“默认选择不同就拒绝旧线程”的上游测试实例，以两个正向流程替代。政策选择变更、政策定义变更、缺少 provider 定义、显式覆盖不能绕过政策，以及损坏配置的测试全部继续运行。

## 必要性

只改调用参数无法越过原生身份比较；统一 provider 名称或为每个 provider 启动独立 app-server 会改变配置及进程模型，不能保留同一服务中旧线程 A 与新线程 B 并存的行为。模型协议适配层不拥有线程恢复与输入准入，不能解决此处限制。

上游 #57840 有意引入该限制，不是一个可直接回移的修复。用户已选择恢复按线程保留 provider 的行为。两个比较集中在同一个配置管理文件，直接删除限制并保留既有校验，比增加配置开关、新抽象或另一份持久化更小，也不要求下游修改请求协议。

该文件在调查时的 upstream/main 最近 90 天有 25 次提交，测试模块入口有 74 次；因此生产逻辑只改现有校验块，新测试放独立文件，入口只增加一行。

## 何时扩展

上游在新输入入口、热恢复、Goal、队列或压缩路径增加默认 provider 身份检查时，核实它是否重新禁止按线程保留 provider。上游改变 saved/requested provider 的解析、管理政策来源或宿主配置加载次数时，重新检查冷恢复的错误优先级与并发恢复边界。

## 何时移除

上游允许默认 provider 改变后已有线程继续使用自己的 provider，或提供覆盖冷恢复、热恢复和全部输入入口的等价能力且用户选择采用时，删除对应改动与 5 个精确跳过项。

## 冲突处理

保留上游新的配置加载、管理政策、provider 存在性和线程生命周期处理，只重新解除默认选择的相等性限制。不要整段恢复旧版配置管理器，也不要删除结构化错误类型。新入口优先复用共享校验；上游已有等价测试时可删除本补丁对应测试，避免重复维护。

## 验证

本地运行 `python3 fork/tools/codeks.py check`、`git diff --check`，并仅格式化本补丁修改的 Rust 文件。按仓库规则不在本地编译或运行 Rust 测试，由发布触发的 `codeks-ci` 编译并执行相关 crate。

新测试以匹配 thread/turn 的完成通知为边界，只统计模型 Responses 请求；断言实际目的地、model、effort、线程身份和历史。确认以下行为：

1. A 的线程已加载时，默认改为 B 后仍可恢复、输入并向 A 发送；随后新建线程使用 B 的默认模型和 effort。
2. A 完成一轮并正常关闭 writer 后，默认 B 的新进程按客户端保存值恢复同一线程并继续向 A 发送；显式指定 A 的 fork 也向 A 发送。
3. 原有政策冲突、provider 定义缺失、损坏系统配置和并发恢复测试仍运行。仅 `ci_skip` 中明确相反的默认选择断言被排除。

## 记录

- 2026-11-01 @ rust-v0.166.0：新增，保留已有线程的 provider 并恢复默认选择独立性。

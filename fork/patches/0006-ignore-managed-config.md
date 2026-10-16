# 0006 忽略系统配置、托管配置和强制约束

```toml
watch_paths = [
  "codex-rs/config/src/loader/**",
  "codex-rs/cloud-config/src/**",
  "codex-rs/config/src/config_layer_source.rs",
  "codex-rs/config/src/config_requirements.rs",
  "codex-rs/config/src/state.rs",
  "codex-rs/exec-server/src/discoverV2/capability_locations.rs",
  "codex-rs/tui/src/startup_preflight.rs",
]
watch_regex = [
  "system_(config|requirements)_toml_file",
  "managed_config_default_path",
  "load_managed_preference|CFPreferences",
  "cloud_config_bundle_loader",
  "RequirementSource::",
  "ConfigLayerSource::(System|Mdm|EnterpriseManaged|LegacyManaged)",
  "/etc/codex|ProgramData",
]
backport_of = []
upstream = []
ci_skip = [
  # 这些上游测试依赖云端配置包，本补丁有意去掉了它，见「改动」
  "package(codex-app-server) & test(=suite::auth::get_auth_status_omits_token_after_proactive_refresh_failure)",
  "package(codex-app-server) & test(=suite::auth::get_auth_status_returns_token_after_proactive_refresh_recovery)",
  "package(codex-app-server) & test(=suite::v2::account::enterprise_tests::logout_reloads_persisted_workspace_and_its_enterprise_policy::current_policy)",
  "package(codex-app-server) & test(=suite::v2::account::enterprise_tests::logout_reloads_persisted_workspace_and_its_enterprise_policy::policy_load_failure)",
  "package(codex-app-server) & test(=suite::v2::account::login_survives_same_owner_token_refresh::refresh_during_requirements)",
  "package(codex-app-server) & test(=suite::v2::account::login_survives_same_owner_token_refresh::refresh_during_account_read_requirements)",
  "package(codex-app-server) & test(=suite::v2::account_system_proxy::browser_login_bootstraps_through_system_proxy)",
  "package(codex-app-server) & test(=suite::v2::thread_fork::thread_fork_surfaces_cloud_config_bundle_load_errors)",
  "package(codex-app-server) & test(=suite::v2::thread_resume::thread_resume_surfaces_cloud_config_bundle_load_errors)",
  "package(codex-app-server) & test(=suite::v2::thread_start::config_requests_surface_cloud_config_bundle_load_errors::model_list)",
  "package(codex-app-server) & test(=suite::v2::thread_start::config_requests_surface_cloud_config_bundle_load_errors::thread_start)",
  "package(codex-app-server) & test(=suite::v2::workspace_routing::failed_workspace_requirements_do_not_fall_back_to_startup_config)",
  "package(codex-app-server) & test(=suite::v2::workspace_routing::login_and_workspace_switch_notify_after_routing_is_ready_then_logout_clears_it)",
  "package(codex-app-server) & test(=suite::v2::workspace_routing::saved_workspace_is_discovered_once_and_not_the_default_account::slow_cloud_bundle_response)",
  "package(codex-cli) & test(=debug_sandbox::cloud_config::tests::debug_sandbox_bootstraps_cloud_managed_permission_profile_from_backend)",
  "package(codex-cli) & test(=add_and_remove_preserve_cloud_managed_resources)",
  "package(codex-cli) & test(=list_and_get_resolve_cloud_managed_mcp_without_writing_user_config)",
  "package(codex-cli) & test(=login_and_logout_persist_only_cloud_managed_mcp_oauth_credentials)",
  "package(codex-cli) & test(=doctor_reports_cloud_filesystem_policy_and_rejects_invalid_requirements)",
  "package(codex-cli) & test(=features_list_honors_cloud_managed_feature_requirements)",
  "package(codex-cli) & test(=sandbox_fetches_and_enforces_cloud_managed_permission_profile)",
  "package(codex-cli) & test(=daemon_worktree_start_and_fork_bind_owner_before_turn)",
  "package(codex-cli) & test(=interactive_worktree_start_and_fork_bind_owner_before_turn)",
  "package(codex-exec) & test(=suite::auth_env::exec_api_key_cannot_discard_stored_workspace_network_policy)",
  "package(codex-exec) & test(=suite::auth_env::exec_bootstrap_rejects_redirected_oauth_and_cloud_policy)",
  "package(codex-exec) & test(=suite::worktree::worktree_start_and_fork_use_host_pool_and_preserve_legacy_resume)",
]
```

## 背景

上游 Codex 支持由系统管理员或企业工作区下发配置。这些来源不在用户的 `CODEX_HOME` 里，却能覆盖或限制用户自己的配置：

| 来源 | 位置 | 作用 |
|---|---|---|
| 系统配置 | `/etc/codex/config.toml`，Windows 是 `%ProgramData%\OpenAI\Codex\config.toml` | 优先级最低的默认值。它所在的目录还会带出 `skills/`、`hooks.json`、`rules/`、`agents/` |
| 强制约束 | 同一目录下的 `requirements.toml` | 限制沙箱、审批策略、登录方式和工作区、model provider、MCP、插件、hooks、exec policy 规则、网络、数据驻留，以及托管的开发者指令等 |
| 旧版托管配置 | `/etc/codex/managed_config.toml` | 优先级最高的配置层，同时转换成审批和沙箱约束，也参与项目根目录和信任的判断 |
| macOS MDM | `com.openai.codex` 的 `config_toml_base64`、`requirements_toml_base64` | 托管配置层和强制约束 |
| 云端配置包 | ChatGPT Business、Edu、Enterprise 账号登录后，从后端的 `/wham/config/bundle` 拉取，缓存在 `CODEX_HOME/cloud-config-bundle-cache.json`，每 15 分钟后台刷新 | 企业托管配置层、强制约束、MCP 的 EMA 策略。拉取失败时启动会失败 |

codeks 是自用的开源 fork，配置应该完全由用户自己决定。同一台机器上的管理员文件、MDM 或登录账号所在工作区的策略，不应该改变 fork 的行为。

## 改动

规则：只加载用户自己的配置，也就是 `CODEX_HOME` 下的 `config.toml` 和 profile、项目的 `.codex/`、命令行的 `-c`、随包的默认值，以及宿主进程传入的 thread config。上表的来源一律不加载。调用方显式传入路径或内容时照常加载，例如上游测试的 fixture，以及 app-server 的调试环境变量 `managed_config_path_from_debug_env`。

- `codex-rs/config/src/loader/mod.rs`
  - `system_config_toml_file_with_overrides` 和 `system_requirements_toml_file_with_overrides` 改为返回 `Option`：`LoaderOverrides` 里没有显式路径时返回 `None`，不再退回 `/etc/codex` 或 ProgramData。签名一变，以后上游新增的调用方在 rebase 后会编译失败，正好提醒我们扩展本补丁。
  - `load_config_layers_state` 没有路径时不建 System 层。`/etc/codex/config.toml` 不读，由 System 层目录带出的 skills、hooks.json、rules、agents 也不再加载。生产代码里只有 `match` 分支和展示文字引用 System 层，没有代码假设它一定存在。
- `codex-rs/config/src/loader/managed_requirements.rs`、`codex-rs/config/src/loader/application.rs`：没有路径时不读 `requirements.toml`。前者用于完整加载，以及只读约束的 `load_managed_requirements_state`；后者用于 app-server 和内嵌客户端的网络约束。
- `codex-rs/config/src/loader/local.rs`：exec-server 读取执行环境本地配置时，同样不读 `requirements.toml`。没有路径时 System 槽位是空表，并且不读文件；exec-server 随后的 `project()` 会丢掉空层，并调整云端层的插入位置。
- `codex-rs/config/src/loader/layer_io.rs`：旧版 `managed_config.toml` 的默认路径在所有平台上都忽略。上游在 Windows 上已经这样处理，补丁只改了非 Windows 的那一行。旧版托管配置带来的配置层、转换出的审批和沙箱约束，以及它对项目发现的影响，都随之消失。
- `codex-rs/config/src/loader/macos.rs`：`load_managed_preference` 直接返回 `None`，同时覆盖 MDM 配置层、MDM 约束和 `has_managed_preferences`。测试用的 base64 覆盖不经过这个函数。CI 只在 Linux 上编译，这一处没有被编译检查过。
- `codex-rs/cloud-config/src/bundle_loader.rs`：三个公开构造函数（`cloud_config_bundle_loader`、`cloud_config_bundle_loader_for_storage`、`cloud_config_bundle_loader_for_storage_without_cache`）直接返回 `CloudConfigBundleLoader::default()`。这和非企业账号在上游的情况一样：不发请求，不读写缓存，不启动后台刷新，也没有 EMA 策略，拉取失败导致启动失败的情况也不会再出现。用常量做提前返回，原来的函数体保持原样，不产生 unreachable 或 dead code 警告。
- `codex-rs/core/tests/suite/cli_stream.rs`：企业账号用个人访问令牌的两个测试，原来期望一次 bundle 请求，改为期望 0 次。这样它们也成了“fork 不拉取云端配置”的端到端检查。
- 元数据 `ci_skip`：codex-app-server、codex-cli、codex-exec 里有 26 个上游测试 mock 了 `/backend-api/wham/config/bundle`，依赖它下发的策略、指令，或者它触发的 token 刷新次数。本补丁去掉了这个请求，它们一定失败。CI 跳过它们（见 0003），不改这些上游测试文件。

有意保留的读取，都不会加载配置内容：

- TUI 首次启动时（`tui/src/startup_orchestration.rs` 调用 `startup_preflight.rs`、`has_local_managed_configuration`）检查 `/etc/codex/config.toml`、`requirements.toml`、`managed_config.toml` 是否存在，只用来决定首屏先显示登录引导还是输入框。
- exec-server 的 discoverV2 会预热 `/etc/codex/skills` 的缓存（`exec-server/src/discoverV2/capability_locations.rs`）。对应的 feature `executor_capability_discovery` 还在开发中，默认关闭。
- Windows 上对系统配置目录的 ACL 探测，只用来告警。
- 账号后端的用户设置（例如 commit attribution）不属于配置层，本补丁不处理。
- 已经存在的 `cloud-config-bundle-cache.json` 不再读取，留在磁盘上也无害。

上游文档里通过 `/etc/codex/config.toml` 设置默认值、通过 `requirements.toml` 或 MDM 限制行为的做法，在 fork 里都不生效。需要默认值时写进 `CODEX_HOME/config.toml`。

## 必要性

少了它，同一台机器上的 `/etc/codex` 文件、MDM，或者登录账号所在工作区的策略，都会覆盖或限制 fork 的配置：沙箱和审批被收紧、登录方式受限、MCP 和插件被禁用、模型请求被改道。用户就不能完全控制自己的 fork。

- 配置做不到：上游没有关闭这些来源的开关。它们本来就是为了让用户关不掉而设计的。
- `LoaderOverrides::ignore_managed_requirements` 是给测试和调试命令用的。它只跳过约束，不管 System 配置层、旧版和 MDM 的配置层、exec-server 的本地读取，云端配置也照样拉取。另外，`LoaderOverrides` 一旦不是默认值，TUI 就把它当作自定义加载器：不再复用本地的 app-server 守护进程（`tui/src/daemon_startup.rs`），带 `--search` 启动时的快速路径也会失效（`tui/src/startup_orchestration.rs`）。所以不能靠改它的默认值。
- 在消费端过滤（只在 `load_config_layers_state` 里丢掉这些层）会漏掉 `load_managed_requirements_state`、application 约束和 exec-server。这些文件也仍然会被读取：文件损坏时照样报错，云端照样发请求。
- 在 `cloud_config_eligible_auth` 里把所有账号当作不符合条件，改动只有一行，但 codex-cloud-config 自己的大部分 service 测试会失败。

## 何时扩展

- 上游新增托管来源：新的 `ConfigLayerSource` 或 `RequirementSource` 变体、`/etc/codex` 或 ProgramData 下的新文件、新的 MDM 键、新的云端构造函数或下发策略的后端接口。按同样的规则让它只接受显式输入。
- 上游新增 `system_*_toml_file_with_overrides` 的调用方：rebase 后会编译失败，按“`None` 就不读”补上。直接调用 `system_config_toml_file()` 或 `system_requirements_toml_file()` 的新代码会被 `watch_regex` 提醒，要看它是不是在加载内容。
- discoverV2 正式启用，或者 TUI 的首次启动判断开始读取文件内容：把它们也纳入本补丁。
- 扩展 CI 里出现新的失败测试，确认原因是它依赖云端配置包、默认系统路径或 MDM：把完整名称加进 `ci_skip`。其他原因的失败要单独处理，不要加进来。

## 何时移除

上游提供官方开关，可以关掉全部系统和托管来源时，改用配置并删掉本补丁。上游删掉某个来源时，删掉对应的那一处。

上游删掉或改名 `ci_skip` 里的测试时，删掉或改正对应的条目。改名后的测试会在扩展 CI 里重新失败，可以由此发现。

## 冲突处理

以上游代码为准，重新套用四条规则：系统 `config.toml`、`requirements.toml` 和 `managed_config.toml` 都没有默认路径；MDM 读取返回空；三个云端构造函数返回空 loader；`cli_stream.rs` 的 bundle 请求期望 0 次。用 `rg 'codeks 0006'` 找到全部落点。

上游测试会显式传入 fixture 路径（`LoaderOverrides::without_managed_config_for_tests`、`with_managed_config_path_for_tests` 等），这些路径照常生效，所以 CI 默认测试的 crate（codex-config、codex-cloud-config、codex-core）里只需要改 `cli_stream.rs` 一处。其他 crate 里依赖云端配置包的测试用 `ci_skip` 跳过，不会和上游的测试文件冲突。

## 验证

- `rg 'codeks 0006' codex-rs` 有 14 处。
- 实机对比（2026-10-15，0006 的开发版构建对比 0005 的构建）：在 `/etc/codex` 放 `config.toml`、`requirements.toml`、`managed_config.toml` 和 `skills/<name>/SKILL.md`，通过 `codex app-server` 调 `config/read`（`includeLayers: true`）、`configRequirements/read` 和 `skills/list`。0005 构建里有 `system` 和 `legacyManagedConfigTomlFromFile` 两层，约束来自这个 `requirements.toml`，skill 以 `admin` 范围出现；0006 构建里只剩用户层，`requirements` 为 `null`，没有这个 skill。`codex exec` 正常完成一轮对话。
  - `/etc/codex` 对本机所有 Codex 进程生效。复测时放无害的内容（例如允许全部沙箱模式、取默认值的配置项），测完立即删除。
- 扩展 CI：`codeks.py ci --crates "codex-config codex-cloud-config codex-core codex-exec codex-cli codex-app-server codex-tui"`。第一次运行时 codex-config、codex-cloud-config、codex-core 全部通过，其余 crate 有 63 个失败：26 个是本补丁的预期结果，现在列在 `ci_skip` 里；37 个是版本号造成的（codex-tui 36 个，codex-cli 的 `packaged_daemon_bootstrap_seeds_local_package`），和本补丁无关，0003 改为用 `0.0.0` 测试后解决。改完后重跑（2026-10-16）：13686 个测试全部通过，其中 1 个 flaky 重试后通过；skipped 从 19 个变成 45 个，多出的正好是 `ci_skip` 里的 26 个。

## 记录

- 2026-10-15 @ rust-v0.162.1：新增。

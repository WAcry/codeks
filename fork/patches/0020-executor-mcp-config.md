# 0020 接入执行环境的 stdio MCP 配置

```toml
watch_paths = ["codex-rs/config/src/mcp_types.rs", "codex-rs/codex-mcp/src/catalog.rs", "codex-rs/codex-mcp/src/server.rs", "codex-rs/codex-mcp/src/plugin_config.rs", "codex-rs/rmcp-client/src/stdio_server_launcher.rs", "codex-rs/rmcp-client/src/utils.rs"]
watch_regex = ["discover_http_mcp_servers|discover_mcp_servers|ExecutorOnly|normalize_stdio|executor.*MCP|MCP.*executor"]
backport_of = []
upstream = ["https://github.com/openai/codex/pull/39941", "https://github.com/openai/codex/pull/48143", "https://github.com/openai/codex/pull/60461"]
ci_skip = []
```

## 背景

上游支持通过执行环境启动 stdio MCP，也支持从选中的执行环境插件发现这类服务；但普通 `config.toml` 的自动发现只接受 HTTP。用户已有的 `command`、`args`、`env_vars` 配置因此不会加入会话能力目录，需要额外包装为插件或 HTTP 服务。

上游 #39941 明确把首次配置发现限定为 HTTP，#48143 随后要求执行环境声明不能在缓存重建、重连时读取控制端凭据。本补丁补齐普通 stdio 入口，保留这个凭据边界。基于 rust-v0.167.1 检查上游后，未发现已经合入的等价实现。

## 改动

- `codex-rs/exec-server/src/environment_config.rs`：把发现入口改为 `discover_mcp_servers`，保留 stdio 条目，在应用执行环境 requirements 前规范化进程上下文。现有 HTTP 能力判断不变。
- `codex-rs/exec-server/src/executor_mcp_config.rs`：新增小型配置规范化模块；`codex-rs/exec-server/src/lib.rs` 注册模块。缺省 cwd 使用选中的执行环境 cwd；相对路径与 `~` 使用执行环境路径约定和用户目录解析，显式 file URI 也可使用。未限定的 `env_vars` 引用绑定为 `source = "remote"`；显式读取控制端变量的声明报错，字面量 `env` 保持原值。指向其他环境的声明仍由已有选择过滤器忽略。
- `codex-rs/core/src/session/mcp_runtime.rs`：将规范化后的 stdio 声明加入已有目录，仍由执行环境选择赋予设备身份。同名控制端配置优先，requirements 和 enabled 保持上游语义。HTTP 环境 headers 与动态 helper 仍走原有排除逻辑。
- `codex-rs/codex-mcp/src/rmcp_client/executor_config.rs`：新增运行时凭据验证；`codex-rs/codex-mcp/src/rmcp_client.rs` 在原来的边界调用它。允许具有 cwd、且变量全部来自执行环境的 stdio 声明，同时保留 `ExecutorOnly`，不放宽缓存身份或控制端凭据回退。HTTP 的既有限制保持不变。
- `codex-rs/app-server/tests/suite/v2/executor_mcp_config_tests.rs`：新增普通配置端到端测试，验证相对和缺省 cwd、执行环境变量、字面量 env、同名控制端优先以及禁用的服务。`codex-rs/app-server/tests/suite/v2/executor_mcp.rs` 注册测试，并将原来“stdio 未发现”的断言改为“stdio 已发现，但被现有控制端 requirements 禁用”；保留原测试的 HTTP 与 requirements 覆盖，不跳过测试。

继续复用原生进程、标准管道和 MCP 生命周期；没有新增执行服务协议、OAuth、进程共享或 stdio 到 HTTP 的代理。此实现运行在控制端，现有具备配置读取和进程能力的执行服务无需同步增加 RPC。后续发版仍应验证新控制端与既有执行服务的组合。

任意 HTTP `env_http_headers` 未包含在本补丁中：除 HTTP 发送层外，认证状态与 OAuth 发现仍存在控制端变量读取路径，需要单独统一其归属，不能只放开过滤条件。

## 必要性

用户已选择原生配置入口，避免每次安装时创建插件包装、重复转换配置或启动额外 HTTP 代理。仅在下游 bridge 转换会产生第二份配置和同步维护，不能覆盖直接使用 codeks 的客户端；新进程机制则重复上游已有能力。

补丁只在发现、规范化和已有凭据校验处挂载，新逻辑放入独立文件。控制端和执行环境的目录、变量分开解释，避免把远端 Windows 路径按控制端系统解析，也避免无意读取同名控制端 token。

## 何时扩展

上游改变执行环境配置发现、MCP 凭据来源、`env_vars` 语义、路径 URI 转换或服务缓存身份时，检查新增类型和生命周期是否仍经过这些边界。#60461 等发现与刷新调整需要同步复查。HTTP headers、OAuth 和共享 stdio 连接属于独立范围，有具体需求时再决定。

## 何时移除

上游普通执行环境配置发现同时支持 stdio、正确规范化 cwd 和变量，并保留执行环境凭据来源后，移除对应实现和挂载点。若上游先增加等价规范化或验证函数，优先改用它们并缩小补丁。

## 冲突处理

保留上游的目录优先级、requirements、HTTP 和认证行为；只恢复带 `codeks 0020` 标记的 stdio 发现和凭据挂载点。不要通过改成 `HostFallbackAllowed` 绕过校验，也不要为了解决冲突移动上游实现。若函数名称变化，检查发现入口所有调用方。原排除 stdio 的测试应继续验证其余 HTTP 与 requirements 行为。

## 验证

本地只检查补丁、diff 和格式，不编译或运行 Rust。CI 运行新增的规范化、凭据检查与 app-server 端到端测试；没有 `ci_skip`。

使用发布版本时，连接有普通 stdio 配置的执行服务，检查模型可以调用工具；确认电脑和控制端存在同名变量时取电脑的值；验证相对、缺省 cwd 与禁用配置；确认同名控制端 MCP 仍优先。跨 Unix/Windows 的路径还由不依赖宿主平台的 URI 单元测试覆盖。已有执行服务组合需要单独做版本兼容验证。

## 记录

- 2026-11-11 @ rust-v0.167.1：接入普通执行环境 stdio 配置，复用启动器并保留执行环境凭据边界；HTTP 环境 headers 留待独立工作。

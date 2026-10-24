# 0012 将 HTTP 模型目录上限提高到 32 MiB

```toml
watch_paths = ["codex-rs/codex-api/src/endpoint/models.rs", "codex-rs/http-client/src/transport.rs", "codex-rs/models-manager/src/manager.rs"]
watch_regex = ["MAX_MODEL_CATALOG_BYTES|response_body_limit_bytes|model_catalog_url|bounded_response_bytes"]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

Codex 0.164.0 对显式 `model_catalog_url` 的完整 HTTP 响应设置 1 MiB 下载上限。目录条目包含完整提示词与能力信息；同一模型的不同部署、上下文窗口和参数预设也各占一个条目。现代模板生成的二十个别名就可能超过上限，导致整份目录无法加载。

需要保留完整条目，并继续通过 HTTP 与 ETag 更新目录。用户已选择在 codeks 提高上限，bridge 完整返回目录，由客户端统一控制下载边界。

## 改动

- `codex-rs/model-provider/src/models_endpoint.rs`：只将 `MAX_MODEL_CATALOG_BYTES` 从 `1024 * 1024` 提高到 `32 * 1024 * 1024`，并挂载独立回归测试。仍使用上游的有界响应读取、JSON 校验、五秒超时和重定向规则。
- `codex-rs/model-provider/src/models_endpoint_catalog_size_tests.rs`：使用内置 GPT-6.1 Sol 元数据构造三十二个独立别名，通过真实本地 HTTP endpoint 下载超过 1 MiB 的目录，逐项验证完整模型元数据没有被裁剪或改写。

上游已有的 `explicit_catalog_rejects_oversized_response` 根据同一常量生成超限输入，因此继续验证超过 32 MiB 时拒绝下载，不改断言、不跳过测试。新的上限允许较多完整预设，不会预先为每次下载分配 32 MiB。

## 必要性

静态 `model_catalog_json` 已能绕过 HTTP 限制，但模型配置变化后需要重新导出文件并重载客户端，不能替代现有 HTTP 发现与 ETag 刷新流程。减少别名或提示词会丢失用户选择或原生模型行为。只修改 bridge 无法越过客户端上限。

选择一个常量的源码补丁，避免引入额外配置、协议扩展或目录裁剪。32 MiB 为完整现代模型条目留出余量，同时保留有限下载边界。bridge 不重复固定客户端上限；无需随这个常量维护第二个拦截点。

## 何时扩展

上游将显式目录限制移到新的传输层、增加分页或按请求缩减目录时，重新检查完整目录是否仍能加载。新增目录入口若另有更低上限，需要先确认实际调用路径再决定是否覆盖。模型管理器若不再共享这条 HTTP 路径，应调整挂点。

## 何时移除

上游默认允许至少 32 MiB，或提供用户愿意使用的等价配置时，移除常量修改和对应回归测试。若上游提供无需裁剪完整元数据的目录分页或共享提示词机制，验证实际预设集合不再触及原上限后，也可移除。

## 冲突处理

保留上游新的鉴权、错误处理、模型管理和传输实现，只重新确认目录下载边界并应用必要常量。不要复制整个 endpoint。若限制已可配置或等价功能已由上游提供，优先移除补丁。

## 验证

本地只运行 `python3 fork/tools/codeks.py check` 与 `git diff --check`；格式化仅针对新增测试文件。远端 `codeks-ci` 验证 `codex-model-provider`，包括超过 1 MiB 的完整目录接受与超过新上限的拒绝。

发布构建后，用 HTTP 目录配置多个模型预设，确认模型列表、提示词、窗口和能力完整，并确认配置变化后的 ETag 仍触发正常刷新。该验证不需要调用真实模型。

## 记录

- 2026-10-24 @ rust-v0.164.0：新增，用户选择保留 HTTP 目录并提高客户端上限。

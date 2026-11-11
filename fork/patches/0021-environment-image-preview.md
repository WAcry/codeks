# 0021 保留图片环境并支持按环境读取文件

```toml
watch_paths = ["codex-rs/app-server/src/notification_media.rs", "codex-rs/exec-server/src/remote_file_system.rs", "codex-rs/utils/path-uri/src/**"]
watch_regex = ["ImageViewItem|ViewImageToolCallEvent|FsReadFileParams|fs/readFile"]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

`view_image` 已能从选中的执行环境读取图片并把字节交给模型，但公开的 `imageView` 条目只保留路径。客户端展开预览时无法判断该路径属于哪个环境；现有 app-server `fs/readFile` 又固定读取 host。两台机器存在相同路径时会读错文件，外部环境离线时也无法明确表达失败。

本补丁只补齐图片预览所需的来源身份和按需读取。用户接受离线、文件删除时预览失败，也接受读取该路径的当前内容，不要求历史快照。Code Mode 已有的内联图片输出不变。

## 改动

- `codex-rs/core/src/tools/handlers/view_image.rs`：新图片事件记录解析后的环境 ID，包括默认 host。
- `codex-rs/protocol/src/items.rs` 和 `codex-rs/protocol/src/protocol.rs`：`ImageViewItem`、`ViewImageToolCallEvent` 增加可选 `environment_id`；旧记录缺少字段时为 `None`，不根据当前线程或路径猜测来源。
- `codex-rs/protocol/src/legacy_events.rs`：生成旧格式事件时保留环境身份。
- `codex-rs/app-server-protocol/src/protocol/v2/item.rs` 和 `codex-rs/app-server-protocol/src/protocol/thread_history.rs`：公开 `imageView` 增加可选 `environmentId`，实时、分页与旧格式历史均透传。
- `codex-rs/app-server-protocol/src/protocol/v2/fs.rs`：`fs/readFile` 增加可选 `environmentId`，返回值仍为 `dataBase64`。路径改用现有 `LegacyAppPathString`，可表达另一操作系统的绝对路径；不改变其他文件系统 RPC。
- `codex-rs/app-server/src/request_processors/fs_processor.rs`：省略环境时保留 host 文件系统和 host 绝对路径校验；指定环境时进入新适配模块。
- `codex-rs/app-server/src/request_processors/fs_environment_read.rs`：按 ID 获取已注册环境，转换其原生绝对路径，并复用现有 executor `fs/readFile`。未知环境、远端读取失败原样成为请求错误，不回退到 host。不新增 executor 协议、文件服务、隧道、快照、自动重放或下载缓存。
- `codex-rs/tui/src/app_server_session/fs.rs`：现有 TUI 文件读取显式省略环境，保持 host 语义。
- `codex-rs/tui/src/dynamic_tools.rs`、`codex-rs/tui/src/chatwidget/replay.rs` 和 `codex-rs/tui/src/thread_transcript/other_items.rs`：仅更新穷尽匹配，保留现有文字展示。
- `codex-rs/app-server-protocol/src/codeks_environment_image_preview_tests.rs` 和 `codex-rs/app-server-protocol/src/lib.rs`：新增并注册实时条目、分页投影、旧事件历史、旧记录缺省值及外部路径契约测试。
- `codex-rs/app-server/tests/suite/v2/codeks_environment_image_preview.rs` 和 `codex-rs/app-server/tests/suite/v2/mod.rs`：用 WebSocket executor fixture 验证 Windows drive/UNC 路径、显式远端路由、默认和显式 host、未知环境与读取错误；验证相对路径不能隐式使用 host cwd。
- `codex-rs/core/tests/suite/view_image.rs`：既有真实工具测试增加开始、结束和旧格式事件的环境身份断言，适配测试选择的本地或远端环境。
- `codex-rs/app-server-protocol/src/protocol/common.rs`、`codex-rs/app-server-protocol/src/protocol/v2/tests.rs`、`codex-rs/app-server/tests/suite/v2/fs.rs`、`codex-rs/app-server/tests/suite/v2/connection_handling_stdio_tests.rs`、`codex-rs/tui/src/chatwidget/tests/helpers.rs` 和 `codex-rs/tui/src/thread_transcript/other_items_tests.rs`：仅给既有 Rust 构造器补充可选字段及路径类型转换，保留原断言与行为。
- `codex-rs/app-server-protocol/schema/json/v2/FsReadFileParams.json`、`codex-rs/app-server-protocol/schema/typescript/v2/FsReadFileParams.ts` 和 `codex-rs/app-server-protocol/schema/typescript/v2/ThreadItem.ts`：同步公开类型导出。

其余生成产物由上游 schema writer 从 Rust 类型导出：

- `codex-rs/app-server-protocol/schema/json/ClientRequest.json`。
- `codex-rs/app-server-protocol/schema/json/ServerNotification.json`。
- `codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.schemas.json`。
- `codex-rs/app-server-protocol/schema/json/codex_app_server_protocol.v2.schemas.json`。
- `codex-rs/app-server-protocol/schema/json/v2/ItemCompletedNotification.json`。
- `codex-rs/app-server-protocol/schema/json/v2/ItemStartedNotification.json`。
- `codex-rs/app-server-protocol/schema/json/v2/ReviewStartResponse.json`。
- `codex-rs/app-server-protocol/schema/json/v2/ThreadForkResponse.json`。
- `codex-rs/app-server-protocol/schema/json/v2/ThreadItemsListResponse.json`。
- `codex-rs/app-server-protocol/schema/json/v2/ThreadListResponse.json`。
- `codex-rs/app-server-protocol/schema/json/v2/ThreadMetadataUpdateResponse.json`。
- `codex-rs/app-server-protocol/schema/json/v2/ThreadReadResponse.json`。
- `codex-rs/app-server-protocol/schema/json/v2/ThreadResumeResponse.json`。
- `codex-rs/app-server-protocol/schema/json/v2/ThreadRevertResponse.json`。
- `codex-rs/app-server-protocol/schema/json/v2/ThreadStartResponse.json`。
- `codex-rs/app-server-protocol/schema/json/v2/ThreadStartedNotification.json`。
- `codex-rs/app-server-protocol/schema/json/v2/ThreadTurnsListResponse.json`。
- `codex-rs/app-server-protocol/schema/json/v2/ThreadUnarchiveResponse.json`。
- `codex-rs/app-server-protocol/schema/json/v2/TurnCompletedNotification.json`。
- `codex-rs/app-server-protocol/schema/json/v2/TurnStartResponse.json`。
- `codex-rs/app-server-protocol/schema/json/v2/TurnStartedNotification.json`。
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-experimental.json.zst`。
- `codex-rs/app-server-protocol/schema/precomputed/app-server-exports-stable.json.zst`。

## 必要性

配置不能补回已经丢弃的事件身份，也不能让 host-only 读文件接口选择其他环境。bridge 不拥有原生文件系统连接。单独增加客户端文件服务会重复进程、端口与连接管理。用户已选择复用现有执行连接，允许按需读取失败，因此在原生事件和 app-server 读取边界做窄补丁即可。

检查当前上游 `ImageViewItem`、`FsReadFileParams`、相关提交和公开搜索结果，未发现等价的环境身份与读文件接口修复。本补丁不扩展命令、patch 元数据，也不改变独立执行服务，部署只需更新 controller/app-server 与消费新字段的客户端。

## 何时扩展

上游增加另一种图片条目或历史表示时，检查环境身份是否仍贯通实时与恢复路径。上游调整原生路径类型或文件读取协议时，优先复用新公共类型和已有 executor 功能。需要永久历史快照或大文件流式下载时应另行设计，不在本补丁里隐式扩大职责。

## 何时移除

上游为图片事件及公共历史条目保留执行环境，并允许 app-server 按该环境读取原生路径时，移除对应落点和兼容构造器改动。若上游已有等价实现，直接采用上游契约，不保留双接口。

## 冲突处理

保留上游新增字段与历史逻辑，只在图片条目的转换边界透传可选身份。新环境读取代码留在独立模块，host 文件系统的其他方法保持上游实现。旧记录不补猜测值；显式环境失败不改读 host。生成导出应从最终 Rust 契约同步，不能只修改 TypeScript 声明。

## 验证

本地执行 `rustfmt`、`git diff --check`、补丁规则检查和源码核对；协议导出使用上游 schema writer 所需的编译与生成入口，不运行单元测试套件。CI 执行新增协议和 app-server 集成测试，以及受影响 crate 的正常测试，不增加测试跳过。

使用发布版本时：在另一个执行环境查看图片，确认实时通知和重新读取的历史都带同一个 `environmentId`；通过 `fs/readFile` 读取该路径，确认内容来自对应环境。用 Windows 原生绝对路径从非 Windows controller 发起读取；省略环境仍读取 host，未知环境和离线环境直接报错。已有 Code Mode 内联图片输出不依赖新读取接口。

## 记录

- 2026-11-11 @ rust-v0.167.1：新增图片环境身份、按环境读文件及跨操作系统路径支持；保留 host 默认与旧历史兼容，不改 executor 协议。

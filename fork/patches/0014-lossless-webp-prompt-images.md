# 0014 优先使用无损 WebP 编码提示图片

```toml
watch_paths = ["codex-rs/core/src/image_preparation.rs", "codex-rs/protocol/src/local_media.rs"]
watch_regex = ["PngEncoder|WebPEncoder|load_for_prompt_bytes|load_data_url_for_prompt"]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

图片以 base64 放入模型请求。重复发送截图历史时，图片字节可能先于 token 数达到请求体积限制。仅调整 PNG 压缩强度不能覆盖原图直通，也不能利用无损 WebP 的编码优势。格式转换应在共享图片入口完成，复用已有缓存和元数据处理。

## 改动

- `codex-rs/utils/image/src/lib.rs`：无需缩放的 PNG 交给独立模块选择更小的无损表示。需要缩放的 PNG 直接生成无损 WebP；其他格式保持原来的编码方式。格式与 data URL 的 MIME 保持一致，尺寸预算和缩放算法保持上游行为。
- `codex-rs/utils/image/src/lossless_webp.rs`：静态 PNG 尝试现有 Rust 无损 WebP 编码器，仅在输出严格更小时替换原字节。保留 RGB ICC 和 EXIF。无需缩放的 16-bit PNG、APNG、超过 WebP 尺寸范围的 PNG，以及无法携带的 ICC 或独立 gamma/色度信息，保留原始文件。只查看 PNG 头部，复用调用方已解码的像素。
- `codex-rs/utils/image/src/lossless_webp_tests.rs`：验证截图体积、含透明像素的逐像素一致性、更小的原始 PNG、元数据，以及需要原字节保留的格式。
- `codex-rs/utils/image/src/image_tests.rs`：更新共享入口的格式断言，继续验证尺寸、原始分辨率模式、data URL、GIF 转换和元数据。
- `codex-rs/core/src/image_preparation_tests.rs`、`codex-rs/core/tests/suite/view_image.rs`、`codex-rs/ext/image-generation/src/tests.rs`：更新附件准备、图片工具和图片编辑引用的预期表示，保留原有路由、内容与来源验证。
- `codex-rs/protocol/src/local_media_tests.rs`、`codex-rs/ext/queue/tests/queue_service.rs`、`codex-rs/ext/history-notes/tests/image_model_input.rs`：更新本地附件快照、队列持久化与编辑、历史图片回放的表示断言，继续验证内容及 detail。
- `codex-rs/core/src/session/tests.rs`、`codex-rs/core/src/session/tests/guardian_tests.rs`：验证待处理用户输入落盘与审核图片使用准备后的表示，用户事件仍验证原始输入。
- `codex-rs/core/tests/suite/client.rs`、`codex-rs/core/tests/suite/responses_lite.rs`、`codex-rs/core/tests/suite/rmcp_client.rs`、`codex-rs/core/tests/suite/truncation.rs`、`codex-rs/core/tests/suite/user_notification.rs`：模型请求及恢复历史比较准备后的图片，保留原始 MCP 事件、通知、detail 与完整输出的断言。
- `codex-rs/core/tests/suite/code_mode.rs`、`codex-rs/core/tests/suite/extension_sandbox.rs`：保留图片像素、证据隔离、恢复及权限断言；证据泄漏检查覆盖所有图片 data URL。
- `codex-rs/core/tests/suite/compact.rs`、`codex-rs/core/tests/suite/compact_remote.rs`：压缩后图片按准备后的表示比较；预算测试使用八张不同的 16-bit 原图 fixture，以连续缓冲区和 Fast/Sub PNG 编码减少构造成本及请求体积，保持尺寸、图片总数、完整顺序比较、两轮裁剪和追加图片断言。图片预算由解码尺寸决定，编码体积不参与；实际 WebP 编码仍由共享图片测试覆盖。
- `codex-rs/core/tests/common/lib.rs`：共享测试 helper 在 `spawn_blocking` 中调用协议快照入口，生成准备后的图片预期，适用于单线程和多线程 Tokio 测试，避免图片缓存的阻塞操作在单线程运行时中触发 panic。
- `codex-rs/app-server/tests/suite/v2/dynamic_tools.rs`、`codex-rs/app-server/tests/suite/v2/turn_start.rs`、`codex-rs/app-server/tests/suite/v2/guardian_v2.rs`、`codex-rs/app-server/tests/suite/v2/imagegen_extension.rs`、`codex-rs/ext/guardian-v2/src/async_scorer/extension_tests.rs`：复用共享 helper 生成测试图片预期，覆盖模型附件、动态工具、审核请求及图片编辑引用；文件保存、原始字节身份和图片文件引用继续独立验证。

无损指对既有处理后的像素进行无损编码；原有缩放及 8-bit 转换的语义不变。无需缩放时比较原 PNG 与 WebP；PNG 缩放时直接编码 WebP，不额外生成 PNG 比较。GIF 等输入转换仍使用现有的 Balanced PNG 编码器。共享缓存避免同一输入和尺寸模式反复编码。历史文件和磁盘上的原图不做批量转换。不新增依赖、配置或传输压缩协议。

## 必要性

用户选择在共享图片入口统一处理，覆盖直接附件、图片工具和使用此入口的扩展。只改某个 UI 会漏掉工具图片；在协议代理中逐轮重编码整段历史会重复工作。现有 `image` crate 已有无损 WebP 编码器，不需要引入外部编码库。独立选择模块只在原图直通入口挂载；重新编码则复用现有 WebP 路径。

提示图片用于 agent 阅读文字和理解界面，用户接受轻微色彩呈现差异，以保持实现精炼和较小的请求体积；像素内容、分辨率及共享编码器支持的 ICC/EXIF 仍按上述规则处理。

## 何时扩展

上游新增绕过共享入口的图片处理方式，或修改元数据、动画、位深及 WebP 编码支持时，核对覆盖范围和无损条件。不同编码器的压缩率不能直接互相推定；增加更慢的编码器前，应以实际截图测量收益和延迟。

## 何时移除

上游共享入口已经采用等价的无损格式选择，并覆盖原图直通和重新编码时，删除本补丁。

## 冲突处理

保留上游的输入验证、图片预算、元数据和错误传播。只重新应用原 PNG 的格式选择挂载点，以及 PNG 重新编码时的 WebP 输出选择。按输出格式更新相关测试，不跳过图片测试。

## 验证

本地运行 `git diff --check`、修改文件的 rustfmt 检查与 `python3 fork/tools/codeks.py check`。GitHub Actions 的 `codeks-ci` 编译客户端，并运行涉及 crate 的测试，包括共享图片入口、附件准备、图片工具和图片编辑引用。

测试必须使用实际 Rust 编码器验证像素、透明通道和元数据，不能用另一种 WebP 实现的测量代替。运行时还应对目标模型确认 WebP 接收能力；本补丁不声称绕过请求上限，也不改变视觉 token 的计量。

## 记录

- 2026-11-09 @ rust-v0.167.0：共享图片入口优先使用无损 WebP；保留更小或不能无损转换的原 PNG。
- 2026-11-10 @ rust-v0.167.0：补齐协议、队列、历史图片及核心调用方的表示断言，隔离预算 fixture 的编码耗时，明确提示图片的色彩保真取舍。

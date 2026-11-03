# 0014 使用平衡模式压缩 PNG

```toml
watch_paths = []
watch_regex = ["PngEncoder::new|PngEncoder::new_with_quality|CompressionType::Default"]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

image 0.25.9 的 PngEncoder::new 默认使用 CompressionType::Fast。统一图片处理在需要缩放 PNG 或将其他格式转换为 PNG 时会重新编码；Fast 可能使图片数据和模型请求体积明显大于使用平衡压缩的结果。不需要重新编码的 PNG/JPEG/WebP 本来就保留原始字节。

## 改动

- codex-rs/utils/image/src/lib.rs：仅将共享 PNG 重编码器改用 PngEncoder::new_with_quality(CompressionType::Default, FilterType::Adaptive)。在当前 image crate 中 Default 映射到 PNG 的 Balanced 压缩；图像仍为无损 PNG，像素、透明通道、缩放策略与 ICC/EXIF 处理不变。
- codex-rs/utils/image/src/image_tests.rs：增加单个回归测试，在具有可压缩性的同像素输入上对比默认 Fast PNG 与 Balanced PNG 的编码字节数，并确认 PNG 解码后的像素相同。

本补丁不修改无需缩放的原图直通、旧会话或历史图片，不修改 JPEG、WebP、GIF、剪贴板、显示预览、图片质量、分辨率及请求结构，也不增加依赖。

## 必要性

没有现成配置可以只控制这一处 PNG 压缩强度。只修改共享编码器一处即可覆盖需要重新编码 PNG 的统一图片处理入口；修改 view_image、TUI 或其他调用方会重复实现且不必要。Balanced 与 Fast 都是无损压缩，但 Balanced 可减小一些图片的请求字节数；付出的是重新编码时的额外 CPU 时间。用户选择只保留这个最小改动，不做颜色通道转换、格式替换或历史重压缩。

## 何时扩展

若上游更换 image/png crate、修改共享 PNG 编码逻辑或 PngEncoder 默认压缩映射，重新确认 Default 仍对应 Balanced；不把本补丁扩展到其他格式，除非有独立需求。

## 何时移除

上游默认 PNG 重编码已经使用同等无损 Balanced 压缩时可移除本补丁。

## 冲突处理

同步上游时保留新的解码、尺寸限制、元数据和错误处理；只在统一 PNG 重编码位置保留 Balanced 选择。无需恢复旧版实现。

## 验证

本地检查 git diff --check、仅改动 Rust 文件的 rustfmt --check 以及 codeks.py check。CI 测试 codex-utils-image 里新加的大小与逐像素回归测试，以及原有源文件字节直通、缩放与元数据保留测试。大小收益随图像内容不同，不能保证每一张 PNG 都更小；本补丁不设体积回退或再编码逻辑。

## 记录

- 2026-11-02 @ rust-v0.166.0：新增，统一 PNG 重编码改用 Balanced 无损压缩。

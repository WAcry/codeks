# 0015 V8 构建资源下载的 GitHub API 兜底

```toml
watch_paths = [
  ".github/scripts/rusty_v8_bazel.py",
  "third_party/v8/**",
  ".github/workflows/codeks-ci.yml",
  ".github/workflows/codeks-npm.yml",
]
watch_regex = ["rusty.v8|setup-rusty-v8|rusty-v8-v"]
backport_of = []
upstream = []
ci_skip = []
```

## 背景

2026-11-08 在 Codeks 0.167.0-codeks.2 上执行 Rust CI 与六平台发布构建时，多个 GitHub Actions runner 在编译前调用 `.github/actions/setup-rusty-v8/action.yml`，通过 `https://github.com/openai/codex/releases/download/rusty-v8-v152.2.0/` 下载预编译 V8 产物时连续收到 HTTP 504。Linux/macOS/Windows 出现相同现象，和本 fork 的 Rust 改动、Exec Server 代码或构建选项无关。

验证相同 tag 在公开的 GitHub Releases REST API 可查询到确切的三个 release assets，`GET /repos/openai/codex/releases/assets/<id>` 携带 `Accept: application/octet-stream` 则可以重定向到可下载的原始资产。构建不应该因为下载展示 URL 的临时故障而中止。

## 改动

- `.github/actions/setup-rusty-v8/action.yml` 新增 `download_release_asset` shell helper：首先对既有浏览器下载 URL 做有限重试；发生错误时读取该官方 release tag 的公开 REST API 元数据，精确按资产名称找到 id，再通过 asset API 下载。第一次回退后，剩余两个资产直接复用同一份 release 元数据与 API 路径；避免对已失效的 URL 重复等待。下载失败或资产名不存在时仍正常报错，绝不静默使用旧版或其他架构的构建产物。
- 原有的两级完整性校验 **不变**：先核对官方 `.sha256` 清单的字节 SHA256 是否等于源码中固定在 `third_party/v8/` 的受信任摘要，再对预编译库和 Rust binding 源文件逐项做 SHA256 校验。两个下载入口得到的文件必须通过同一检查。
- 通过公开的 GitHub API 访问官方公开资产，不依赖 GitHub Actions token、私有下载镜像、第三方资源或额外软件包；仍使用上游已有的 `curl`、`bash`、`python3`。只处理资源获取方式，不修改 Rust 代码、沙箱能力、版本标识或发布内容。

## 必要性

V8 原生库是 Codeks CLI、Exec Server 和 CI 编译的必需输入。直接重新运行 workflow 只能寄希望于 GitHub 展示 URL 自行恢复；持续 504 会使所有平台同时失去发布能力。与增加镜像、关闭校验或将旧版构建产物硬编码到新 release 相比，官方同一资产的公开 API 回退修改范围更小，也保留了仓库原有可复现性和信任链。

## 何时扩展

- 上游更改 `.github/actions/setup-rusty-v8/action.yml` 的下载接口、release 命名、manifest 格式，或 `third_party/v8/` 中的 pinned V8 版本时，确认 REST API 资产名仍匹配、校验逻辑没有被绕过。
- 如果 GitHub 修改 release asset REST API 或停止对公开资产提供未经身份认证的重定向，优先更新到官方支持的访问机制，再考虑复用 Actions cache；不能跳过 SHA256。

## 何时移除

若上游加入可靠的等价多入口下载、合理重试或缓存机制，可删除本 helper 并采用上游实现；如果所有 Codeks workflow 不再依赖该 action，也可以移除。

## 冲突处理

保留上游最新 V8 版本、目标三元组、预编译产物和两层校验作为事实来源，只把 API 回退放在实际下载点。遇到参数或校验形式变化，应优先适配上游的新下载路径，不能保留已经过时的发布 tag 或资产名。

## 验证

- 对复合 action 的 shell 运行 `bash -n`，确认 Bash 解析通过；用公开 release API 的真实响应核对资产名称与 id 的解析。
- 原 URL 正常时继续使用原行为；遇到 504 时回退 API 下载，校验 `.sha256` 清单、预编译库和 binding；HTTP 错误、资产缺失或摘要错误均须停止构建。
- 在 `codeks-ci` 和 `codeks-npm` 的 Linux、macOS、Windows runner 上执行原有构建，不新增测试跳过项。

## 记录

- 2026-11-08 @ rust-v0.167.0：新增。处理上游 GitHub Releases 浏览器下载 URL 返回 504、同一资产 API 可以获取的构建故障。

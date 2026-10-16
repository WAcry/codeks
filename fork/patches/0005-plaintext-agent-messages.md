# 0005 multi-agent v2：自定义 namespace 下的消息工具改用明文

```toml
watch_paths = [
  "codex-rs/core/src/tools/handlers/multi_agents_spec.rs",
  "codex-rs/core/src/tools/handlers/multi_agents_v2.rs",
  "codex-rs/core/src/tools/handlers/multi_agents_v2/**",
  "codex-rs/core/src/agent/control/delivery.rs",
  "codex-rs/core/src/tools/spec_plan.rs",
]
watch_regex = [
  '\.with_encrypted\(',
  "encrypted_function_args",
  "DirectPlaintextMessage",
  "DEFAULT_MULTI_AGENT_V2_TOOL_NAMESPACE",
]
backport_of = []
upstream = []
```

## 背景

multi-agent v2 的 `spawn_agent`、`send_message`、`followup_task` 三个工具，在参数 schema 里把 `message` 标成 `"encrypted": true`（`codex-rs/core/src/tools/handlers/multi_agents_spec.rs` 里的 `with_encrypted()`）。服务端看到这个标记，会把模型写的 `message` 加密后放进 `function_call` 的参数。Codex 不解密，把密文作为 `encrypted_content` 交给子 agent，由服务端在子 agent 的请求里解密。只有响应里带 `encrypted_function_args: []`，并且 namespace 是 `collaboration` 时，Codex 才按明文投递（上游提交 03edf16f0，Support plaintext collaboration tool messages）。

这套协议有三个问题：

1. 不是所有上游都支持。2026-10-15 通过某个 OpenAI 兼容服务实测：AWS 上的 gpt-6.1-sol 把 `message` 加密成 `pv_…`，子 agent 只收到一串密文，回复 “Received the encoded payload.”。
2. 我们想看到 spawn 的 prompt 明文。现在父 agent 的历史、日志和界面里只有密文。
3. 以后可能接入不认识这个标记的模型，例如通过 LiteLLM 接入 Claude。

只去掉 `encrypted` 标记不够。同一天直接调用 Responses 接口实测，Azure 的 gpt-5.6-sol 和 AWS 的 gpt-6.1-sol 结果相同：

| namespace | 带 `encrypted` 标记 | 结果 |
|---|---|---|
| `collaboration` | 是 | 200，`message` 是密文（Azure 是 `gAAAAAB…`，AWS 是 `pv_…`） |
| `collaboration` | 否 | 400：`Function 'collaboration.spawn_agent' is reserved for use by this model and must match the configured schema.` |
| `agents` | 是 | 200，`message` 仍是密文 |
| `agents` | 否 | 200，`message` 是明文，响应里没有 `encrypted_function_args` |

结论：`collaboration` 是服务端保留的内置工具，schema 必须带加密标记；加密只由标记触发，和 namespace 无关。要拿到明文，必须换 namespace，同时去掉标记。

namespace 本来就能配置（`features.multi_agent_v2.tool_namespace`，默认 `collaboration`）。但上游在自定义 namespace 下仍然发送加密标记，并且只在 `collaboration` 下认可明文，所以只改配置不够。

## 改动

规则：multi-agent v2 的 namespace 配成 `collaboration` 以外的值时，三个消息工具使用明文。默认配置不受影响。

启用方式：

```toml
[features.multi_agent_v2]
enabled = true
tool_namespace = "agents"
```

涉及的文件：

- `codex-rs/core/src/tools/multi_agent_tool.rs`
  - 新增 `uses_plaintext_messages(namespace)`：namespace 存在并且不是 `collaboration` 时返回 true。规则只写在这一处。
  - `MultiAgentV2ToolOverrides::spec()` 在套用 catalog 的描述和参数覆盖之后，如果是自定义 namespace，就清掉所有顶层参数的 `encrypted` 字段。配置了 namespace 时，multi-agent v2 工具的 schema 都由这个 `spec()` 生成，所以请求里不会再有加密标记。catalog 参数缺少 `message` 时退回内置 schema 的逻辑保持上游原样。
- `codex-rs/core/src/tools/router.rs` 的 `ToolCall::direct_source()`：上游只把 `collaboration` 下 `encrypted_function_args == Some([])` 的三个消息工具当作明文。补丁让自定义 namespace 下 `encrypted_function_args` 缺失或为空都算明文；服务端明确列出加密参数时仍按密文处理。这样 `agent_message_from_tool` 产生 `AgentMessage::Plaintext`，子 agent 收到 `Message Type: NEW_TASK … Payload:\n<明文>` 形式的 `input_text`。这些调用的参数在日志里显示为 `[plaintext arguments]`，和上游处理明文消息的方式一致。
- `codex-rs/core/tests/suite/multi_agent_tool_descriptions.rs`：这个上游测试用自定义 namespace `delegation`，原来期望 catalog 参数里的 `message` 带 `encrypted: true`。补丁追加一段：除了不带 namespace 的 `Exposure::Plain`，其他情况期望没有 `encrypted` 字段。上游原有的断言行没改。
- `codex-rs/core/tests/suite/codeks_plaintext_agent_messages.rs`（新文件）和 `codex-rs/core/tests/suite/mod.rs`（加一行 `mod`）：namespace 设为 `agents`，检查父请求里三个消息工具的 `message` 没有 `encrypted`，子 agent 收到的 `agent_message` 是明文。

副作用：`direct_source()` 看不到配置，只按 namespace 和工具名判断。别的 namespace（例如某个 MCP 服务器）里如果恰好有同名工具，它的参数在日志里也会显示为 `[plaintext arguments]`。执行逻辑不受影响，默认配置下只有这一点差别。

参考：某个下游项目曾用代理在请求和响应之间做同样的转换，请求里把 `collaboration` 改名为 `agents` 并删掉 `encrypted`，响应里改回来并补上 `encrypted_function_args: []`。在 Codex 里直接配置 namespace，工具定义、历史、tool search 输出和 turn metadata 里的名字天然一致，不需要来回改名。

不在范围内：

- history notes 系列工具也用服务端加密。它还在开发中，只支持 ChatGPT 订阅，默认关闭，本补丁不处理。
- code mode：默认 `non_code_mode_only = true`，multi-agent v2 工具不进 code mode。改成 false 时，code mode 里发出的消息仍按上游的密文路径投递，上游在 `collaboration` 下也是这样。

## 必要性

少了它，即使把 namespace 配成 `agents`：请求里仍带加密标记，服务端照样加密；响应里没有 `encrypted_function_args: []`，Codex 也会按密文投递。在不支持这套协议的上游（例如目前的 AWS），子 agent 读不到任务；在所有上游，spawn 的 prompt 都只能看到密文。

- 只改配置不够：namespace 能配置，但加密标记和明文判定写在代码里。
- 适配层（代理）能做到，但要多部署一个服务，还要在请求和响应两边维护改名，历史和 tool search 里的名字也要一起改。
- 把 fork 的默认 namespace 改成 `agents` 会让上游约 46 个测试文件里依赖 `collaboration` 和加密投递的断言失败，冲突面太大。所以默认值不变，由配置开启。

## 何时扩展

- 上游给别的工具加上服务端加密（新的 `with_encrypted()` 或 `encrypted_function_args` 用法）：先和用户讨论要不要也改成明文，不要直接扩展本补丁。
- 上游改了明文判定或投递方式（`direct_source`、`DirectPlaintextMessage`、`agent_message_from_tool`、`AgentMessage`、`delivery.rs`）：确认自定义 namespace 下仍然按明文投递。
- 上游改了 schema 的生成或 catalog 覆盖（`multi_agent_v2_handler`、`MultiAgentV2ToolOverrides::spec()`）：确认清除标记仍在覆盖之后执行。
- 上游新增带 `message` 的 multi-agent 工具：`direct_source()` 的工具名单要跟着上游一起更新。
- 上游改了 namespace 配置（字段名、默认值、校验规则）：更新上面的启用方式。

## 何时移除

- 上游提供了关闭参数加密的配置，或者在自定义 namespace 下默认使用明文：改用上游的方式，删掉本补丁。
- 用到的上游都支持服务端加密，并且不再需要看 spawn 的 prompt 明文：可以删掉。

## 冲突处理

以上游代码为准，重新套用两条规则：自定义 namespace 下 schema 不带 `encrypted`；自定义 namespace 下三个消息工具在 `encrypted_function_args` 缺失或为空时按明文投递。用 `rg 'codeks 0005'` 找到全部落点。上游测试冲突时以上游的新断言为准，再按同样的规则调整期望值。

## 验证

- 不编译的检查：`rg 'codeks 0005' codex-rs` 有 6 处；`rg -n '\.with_encrypted\(' codex-rs/core/src` 只有三个消息工具的 3 处，多出来的就是上游新增的加密工具，见「何时扩展」。
- 用上新版本后：配置 `tool_namespace = "agents"`，在 Azure 和 AWS 的模型上各 spawn 一个子 agent，再用 `send_message` 和 `followup_task` 各发一次。父 agent 的工具调用参数是明文，子 agent 按任务回复，没有 400。
- 切换配置之前的会话可以继续：历史里 `collaboration` 下的旧调用（参数是密文）配上 `agents` 的工具定义，两个上游都接受（2026-10-15 实测）。

## 记录

- 2026-10-15 @ rust-v0.162.1：新增。

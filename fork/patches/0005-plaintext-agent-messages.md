# 0005 multi-agent v2：自定义 namespace 下的消息工具改用明文

```toml
watch_paths = [
  "codex-rs/core/src/tools/handlers/multi_agents_spec.rs",
  "codex-rs/core/src/tools/handlers/multi_agents_v2.rs",
  "codex-rs/core/src/tools/handlers/multi_agents_v2/**",
  "codex-rs/core/src/agent/control/delivery.rs",
  "codex-rs/core/src/thread_spawn.rs",
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

配置要求：用到下面任一场景时，必须把 namespace 配成 `agents`，否则子 agent 收到的仍是密文：

- 上游不支持参数加密，例如目前的 AWS。
- 会话里会用到非 GPT 模型，例如通过 codeks-bridge 接入的 Claude 或 DeepSeek。GPT 父 agent 写出的密文，其他模型的子 agent 读不懂。
- 想在历史、日志和界面里看到 spawn 的 prompt 明文。

namespace 是全局配置，对所有模型生效。模型目录只能覆盖这些工具的描述和参数，不能按模型设置 namespace（`codex-rs/protocol/src/openai_models.rs` 的 `MultiAgentToolMessages`）。所以所有模型共用 `agents`，切换模型时工具名和投递方式都不变。codeks-bridge 生成的配置片段已经包含这一项。

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
- `codex-rs/core/src/tools/handlers/multi_agents_v2.rs` 的 `agent_message_from_tool()`：Code Mode 内的调用也按同一 namespace 规则分类。`codex-rs/core/src/tools/handlers/multi_agents_v2/spawn.rs` 和 `codex-rs/core/src/tools/handlers/multi_agents_v2/message_tool.rs` 传入实际调用的 namespace；后者同时覆盖 `send_message` 和 `followup_task`。保留 `ToolCallSource::CodeMode` 及其 cell ID、嵌套调用 ID，只改变消息内容的明文／密文分类，避免影响调用追踪和历史关联。
- `codex-rs/core/tests/suite/multi_agent_tool_descriptions.rs`：这个上游测试用自定义 namespace `delegation`，原来期望 catalog 参数里的 `message` 带 `encrypted: true`。补丁追加一段：除了不带 namespace 的 `Exposure::Plain`，其他情况期望没有 `encrypted` 字段。上游原有的断言行没改。
- `codex-rs/core/tests/suite/codeks_plaintext_agent_messages.rs`（新文件）和 `codex-rs/core/tests/suite/mod.rs`（加一行 `mod`）：namespace 设为 `agents`，检查父请求里三个消息工具的 `message` 没有 `encrypted`，子 agent 收到的 `agent_message` 是明文。直接调用用例保留默认配置；Code Mode 用例显式设为 `false`，通过 `exec` 覆盖 spawn、send 和 followup，检查接收方三条消息均为 `input_text` 并保留原文。

副作用：`direct_source()` 看不到配置，只按 namespace 和工具名判断。别的 namespace（例如某个 MCP 服务器）里如果恰好有同名工具，它的参数在日志里也会显示为 `[plaintext arguments]`。执行逻辑不受影响，默认配置下只有这一点差别。

上游的多代理提示词（`codex-rs/prompts/src/multi_agent_instructions.rs`）里有一个写死的例子 `to=functions.collaboration.spawn_agent`。同一句要求按工具定义里显示的名字调用，所以配置 `agents` 后不需要改提示词。

`non_code_mode_only` 保持上游默认值 `true`，现有配置也不改。默认情况下，代理工具只供模型直接调用；即使模型采用 Code Mode Only，这些工具也通过 `DirectModelOnly` 保留为直接调用。只有未来显式配置 `non_code_mode_only = false` 时，JavaScript 内的代理调用才会用到新增分支。Code Mode 的工具定义同样来自 `spec()`，已由原有逻辑清除加密标记，无需另一套 schema 转换。

参考：某个下游项目曾用代理在请求和响应之间做同样的转换，请求里把 `collaboration` 改名为 `agents` 并删掉 `encrypted`，响应里改回来并补上 `encrypted_function_args: []`。在 Codex 里直接配置 namespace，工具定义、历史、tool search 输出和 turn metadata 里的名字天然一致，不需要来回改名。

不在范围内：

- history notes 系列工具也用服务端加密。它还在开发中，只支持 ChatGPT 订阅，默认关闭，本补丁不处理。

## 必要性

少了它，即使把 namespace 配成 `agents`：请求里仍带加密标记，服务端照样加密；响应里没有 `encrypted_function_args: []`，Codex 也会按密文投递。在不支持这套协议的上游（例如目前的 AWS），子 agent 读不到任务；在所有上游，spawn 的 prompt 都只能看到密文。

Code Mode 的嵌套调用不经过 `direct_source()`。如果缺少 `agent_message_from_tool()` 中的分支，未来显式设置 `non_code_mode_only = false` 后，JavaScript 传入的明文仍会被包装成密文，工具定义与投递方式不一致。这里补齐分类，不改变默认工具暴露方式。

- 只改配置不够：namespace 能配置，但加密标记和明文判定写在代码里。
- 适配层（代理）能做到，但要多部署一个服务，还要在请求和响应两边维护改名，历史和 tool search 里的名字也要一起改。
- 把 fork 的默认 namespace 改成 `agents` 能省掉一行配置，但冲突面太大。上游大量测试依赖默认的 `collaboration` 和加密投递。2026-10-18 静态核查：core 的集成测试和 `spec_plan_tests.rs` 可以在测试辅助代码里统一固定成 `collaboration`；app-server 和 exec 的测试各自写 `config.toml` 再启动二进制，约 15 个文件要逐个改。所以默认值不变，由配置开启。

## 何时扩展

- 上游给别的工具加上服务端加密（新的 `with_encrypted()` 或 `encrypted_function_args` 用法）：先和用户讨论要不要也改成明文，不要直接扩展本补丁。
- 上游改了明文判定或投递方式（`direct_source`、`DirectPlaintextMessage`、`agent_message_from_tool`、`AgentMessage`、`delivery.rs`）：确认自定义 namespace 下仍然按明文投递。
- 上游改了 Code Mode 的调用来源、工具定义或参数加密元数据：重新核对嵌套调用的消息分类，保留来源信息；若开始提供显式加密参数清单，按新的协议判断。
- 上游改了 schema 的生成或 catalog 覆盖（`multi_agent_v2_handler`、`MultiAgentV2ToolOverrides::spec()`）：确认清除标记仍在覆盖之后执行。
- 上游新增带 `message` 的 multi-agent 工具：`direct_source()` 的工具名单要跟着上游一起更新。
- 上游改了 namespace 配置（字段名、默认值、校验规则）：更新上面的配置要求和启用方式。
- 上游允许模型目录按模型设置 namespace：确认所有模型仍然用同一个非 `collaboration` 的 namespace。
- 上游测试不再依赖默认 namespace（例如统一通过测试辅助代码配置）：重新评估把 fork 的默认值改成 `agents`，省掉这行配置。

## 何时移除

- 上游提供了关闭参数加密的配置，或者在自定义 namespace 下默认使用明文：改用上游的方式，删掉本补丁。
- 用到的上游都支持服务端加密，并且不再需要看 spawn 的 prompt 明文：可以删掉。

## 冲突处理

以上游代码为准，重新套用两条规则：自定义 namespace 下 schema 不带 `encrypted`；自定义 namespace 下三个消息工具的直接调用在 `encrypted_function_args` 缺失或为空时按明文投递，Code Mode 内的调用同样按明文投递。保留 Code Mode 来源和 `non_code_mode_only` 的默认值。用 `rg 'codeks 0005'` 找到全部落点。上游测试冲突时以上游的新断言为准，再按同样的规则调整期望值。

## 验证

- 不编译的检查：用 `rg 'codeks 0005' codex-rs` 核对补丁落点；`rg -n '\.with_encrypted\(' codex-rs/core/src` 只有三个消息工具的 3 处，多出来的就是上游新增的加密工具，见「何时扩展」。
- 用上新版本后：配置 `tool_namespace = "agents"`，在 Azure 和 AWS 的模型上各 spawn 一个子 agent，再用 `send_message` 和 `followup_task` 各发一次。父 agent 的工具调用参数是明文，子 agent 按任务回复，没有 400。
- Code Mode 路径：在测试配置里显式设置 `non_code_mode_only = false`，从 JavaScript 调用代理工具，检查接收方 `agent_message` 使用 `input_text`；常用配置继续保持默认值 `true`。
- 切换配置之前的会话可以继续：历史里 `collaboration` 下的旧调用（参数是密文）配上 `agents` 的工具定义，两个上游都接受（2026-10-15 实测）。

## 记录

- 2026-10-15 @ rust-v0.162.1：新增。
- 2026-10-17 @ rust-v0.163.0：同步复核，保留原有代码。上游 #53660 把公共 spawn 流程移到 `codex-rs/core/src/thread_spawn.rs`，仍原样传递已分类的 `AgentMessage`，因此增加该路径的监控。schema 覆盖、明文识别和投递规则未变，加密参数仍仅有三个消息工具的三处。上游 #52535 新增默认关闭的 `preserve_fork_prefix`，不改变本补丁；#52742 新增默认关闭、仅 OpenAI provider 启用的输出密文回放，自定义 provider 会剥除新增输出密文，不扩展本补丁的工具加密范围。
- 2026-10-18 @ rust-v0.163.0：文档写明哪些场景必须配置 `agents`，以及所有模型共用这一项配置。重新评估了把默认值改成 `agents` 的测试冲突面，维持由配置开启。代码未变。
- 2026-10-18 @ rust-v0.163.0：补齐自定义 namespace 下 Code Mode 代理消息的明文分类；保持 `non_code_mode_only = true` 的默认值和现有配置，为未来显式开启嵌套调用准备。

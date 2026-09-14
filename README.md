# Ferrin

![Ferrin：AI, in Rust. One API. Multiple providers.](assets/banner.png)

[![ci](https://github.com/f4tumnigrum/ferrin/actions/workflows/ci.yml/badge.svg)](https://github.com/f4tumnigrum/ferrin/actions/workflows/ci.yml)
[![rust 1.98+](https://img.shields.io/badge/rust-1.98%2B-orange.svg)](rust-toolchain.toml)
[![license](https://img.shields.io/badge/license-Apache--2.0-blue.svg)](#许可)

Ferrin 是一个 Rust AI SDK。它用一套与供应商无关的接口调用大语言模型：文本生成、流式输出、带审批的工具调用、Agent 循环、结构化输出，以及嵌入、图像、语音、转写、重排、视频等其他模态；内置 MCP 客户端和 OpenTelemetry 导出。第一方供应商有 OpenAI、Anthropic、Google Generative AI 和任意 OpenAI 兼容端点。

项目处于 0.1.0 开发阶段，尚未发布到 crates.io，公共 API 可能变化。当前状态见[项目状态](#项目状态)。

## 特性

- **统一的模型接口**：`generate_text` / `stream_text` 对所有供应商使用同一套构建器，切换供应商只需换模型句柄。
- **工具调用**：`#[ferrin::tool]` 把异步函数变成带 JSON Schema 的工具；多步工具循环、`stop_when` 停止条件、供应商执行的工具、动态工具。
- **人在回路审批**：工具可标记为需要审批，审批请求带 HMAC 签名，审批结果作为消息回传。
- **结构化输出**：`Output::<T>::object()` 让模型直接填充带 `JsonSchema` 的 Rust 类型，流式时可获得部分对象与数组元素。
- **Agent**：`ToolLoopAgent` 封装模型、指令、工具与停止条件，可复用、可挂钩每一步。
- **流式管线**：事件流与最终结果分离，文本流、平滑输出、原始分块透传，可直接转发为 SSE。
- **其他模态**：嵌入、图像、语音合成、转写、语音翻译、重排、视频、文件与技能上传、批处理、实时会话。
- **MCP 客户端**：Streamable HTTP、SSE 与 stdio 传输，OAuth 授权，服务器工具一键接入工具集。
- **可观测性**：`tracing` span 遵循 OpenTelemetry GenAI 语义约定；`ferrin-otel` 导出 span 与指标。
- **工程约束**：无 `unsafe`，库代码禁止 `unwrap`，全部 HTTP 经统一传输层，密钥使用 `secrecy` 类型且不进日志。

## 快速开始

需要 Rust 1.98 及以上。Ferrin 尚未发布，请以 git 依赖引入：

```toml
[dependencies]
ferrin = { git = "https://github.com/f4tumnigrum/ferrin", features = ["openai"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

设置 `OPENAI_API_KEY` 后运行：

```rust
use ferrin::openai::{create_openai, OpenAiSettings};
use ferrin::prelude::*;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let openai = create_openai(OpenAiSettings::default())?; // reads OPENAI_API_KEY
    let result = generate_text(openai.responses("gpt-5"))
        .system("You are a concise assistant.")
        .prompt("Explain backpressure in two sentences.")
        .await?;
    println!("{}", result.text());
    println!("{:?} output tokens", result.usage().output.total);
    Ok(())
}
```

`ferrin::prelude` 导出常用条目：入口函数、`Tool`/`ToolSet`、`Message`、`StreamEvent`、`Output`、`step_count`，以及 `serde`、`schemars`、`json!` 和 `StreamExt`。

## 用法

### 流式输出

```rust
let stream = stream_text(openai.responses("gpt-5"))
    .prompt("Write a haiku about ownership.")
    .await?;

// Text deltas only.
let mut text = std::pin::pin!(stream.text_stream());
while let Some(delta) = text.next().await {
    print!("{}", delta?);
}
```

需要完整事件时把结果拆成事件流和完成句柄：

```rust
let (mut events, completion) = stream.split();
while let Some(event) = events.next().await {
    match event {
        StreamEvent::TextDelta { text, .. } => print!("{text}"),
        StreamEvent::ToolCall(call) => println!("tool call: {}", call.tool_name),
        _ => {}
    }
}
let result = completion.await?;
println!("{} steps", result.steps.len());
```

### 工具调用

```rust
#[derive(Deserialize, JsonSchema)]
#[serde(crate = "ferrin::serde")]
#[schemars(crate = "ferrin::schemars")]
struct GetWeather {
    /// City name.
    city: String,
}

/// Returns the current weather for a city.
#[ferrin::tool]
async fn get_weather(input: GetWeather) -> Result<JsonValue, ToolError> {
    Ok(json!({ "city": input.city, "temperature_c": 18.5 }))
}

let tools = ToolSet::new().insert("get_weather", get_weather())?;
let result = generate_text(openai.responses("gpt-5"))
    .prompt("What is the weather in Berlin?")
    .tools(tools)
    .stop_when(step_count(3)) // up to three model calls in one invocation
    .await?;
```

函数的文档注释成为工具描述，输入类型的 `JsonSchema` 成为参数 schema。工具在同一次调用内自动执行，结果回传给模型，直到模型给出最终回答或触发停止条件。

### 结构化输出

```rust
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(crate = "ferrin::serde")]
#[schemars(crate = "ferrin::schemars")]
struct Recipe {
    name: String,
    ingredients: Vec<String>,
    minutes: u32,
}

let result = generate_text(openai.responses("gpt-5"))
    .prompt("Give me a vegetarian lasagna recipe.")
    .output(Output::<Recipe>::object())
    .await?;
let recipe: &Recipe = &result.output;
```

### Agent

```rust
let agent = ToolLoopAgent::builder(openai.responses("gpt-5"))
    .id("weather-assistant")
    .instructions("You answer weather questions. Use the tools.")
    .tools(ToolSet::new().insert("get_weather", get_weather())?)
    .stop_when(step_count(6))
    .on_step_end(|step: Arc<StepResult>| async move {
        for call in step.tool_calls() {
            println!("-> {}({})", call.tool_name, call.input);
        }
    })
    .build();

let result = agent
    .generate(AgentCall::prompt("What is the weather in Berlin and Tokyo?"))
    .await?;
```

### 工具审批

```rust
let delete_file = Tool::function::<DeleteFile>()
    .description("Deletes a file from the workspace.")
    .needs_approval(NeedsApproval::Always)
    .execute(|input: DeleteFile, _ctx: ToolContext| async move {
        Ok::<_, ToolError>(json!({ "deleted": input.path }))
    })
    .build();
let tools = ToolSet::new().insert("delete_file", delete_file)?;

let mut messages = vec![Message::user("Delete build/cache.bin.")];
let first = generate_text(openai.responses("gpt-5"))
    .messages(messages.clone())
    .tools(tools.clone())
    .await?;
messages.extend(first.response_messages());
for request in first.last_step().tool_approval_requests() {
    // Ask the operator, then record the decision.
    messages.push_approval_response(ToolApprovalResponse::approved(request.approval_id.clone()));
}

let second = generate_text(openai.responses("gpt-5"))
    .messages(messages)
    .tools(tools)
    .await?; // executes the approved call and finishes the answer
```

### MCP

```rust
use ferrin::mcp::{McpClient, McpClientConfig, ToolsOptions};
use ferrin::mcp::transport::TransportConfig;

let transport = TransportConfig::http(url::Url::parse("https://mcp.example.com/mcp")?);
let client = McpClient::connect(McpClientConfig::new(transport)).await?;
let tools = client.tools(ToolsOptions::default()).await?; // ToolSet backed by the server

let result = generate_text(openai.responses("gpt-5"))
    .prompt("Use the server's tools to sum 21 and 21.")
    .tools(tools)
    .stop_when(step_count(4))
    .await?;
client.close().await?;
```

需要 feature `mcp`。stdio 传输用 `TransportConfig::stdio(command)`，见 `examples/example-mcp`。

### 其他供应商

```rust
use ferrin::anthropic::{create_anthropic, AnthropicSettings};
use ferrin::google::{create_google, GoogleSettings};
use ferrin::openai_compatible::{create_openai_compatible, OpenAiCompatibleSettings};

let anthropic = create_anthropic(AnthropicSettings::default())?; // ANTHROPIC_API_KEY
let google = create_google(GoogleSettings::default())?; // GOOGLE_GENERATIVE_AI_API_KEY
let mut settings =
    OpenAiCompatibleSettings::new("local", url::Url::parse("http://localhost:11434/v1")?);
settings.api_key_env = Some("LOCAL_API_KEY".to_owned());
let local = create_openai_compatible(settings)?;

let claude = anthropic.messages("claude-sonnet-4-5");
let gemini = google.chat("gemini-2.5-flash");
let llama = local.chat("llama3");
```

模型句柄都实现 `LanguageModel`，可以直接传给 `generate_text`、`stream_text` 或 `ToolLoopAgent::builder`。

### 错误处理

```rust
match generate_text(openai.responses("gpt-5")).prompt("hi").await {
    Ok(result) => println!("{}", result.text()),
    Err(error) if error.is_retryable() => eprintln!("transient: {error}"),
    Err(error) => return Err(error.into()),
}
```

`ferrin::Error` 提供 `kind()`、`status_code()` 与 `is_retryable()`；可重试的供应商错误默认按指数退避重试，策略由 `RetryPolicy` 调整。

## 供应商

| 供应商 | crate / feature | 环境变量 | 能力 |
| --- | --- | --- | --- |
| OpenAI | `ferrin-openai` / `openai` | `OPENAI_API_KEY`、`OPENAI_BASE_URL` | Responses、Chat Completions、Completions、嵌入、图像、语音、转写、语音翻译、文件、技能、批处理、实时会话 |
| Anthropic | `ferrin-anthropic` / `anthropic` | `ANTHROPIC_API_KEY`、`ANTHROPIC_BASE_URL` | Messages（工具、结构化输出、扩展思考、引用）、文件上传、技能、批处理 |
| Google Generative AI | `ferrin-google` / `google` | `GOOGLE_GENERATIVE_AI_API_KEY` | `generateContent`、嵌入、图像、语音、转写、视频、文件、批处理、Live API 会话 |
| OpenAI 兼容端点 | `ferrin-openai-compatible` / `openai-compatible` | 由设置指定 | Chat Completions、Completions、嵌入、图像 |

每个供应商的完整能力矩阵、设置项与供应商选项见 `docs/providers/`：[OpenAI](docs/providers/openai.md)、[Anthropic](docs/providers/anthropic.md)、[Google](docs/providers/google.md)、[OpenAI 兼容端点](docs/providers/openai-compatible.md)。实现新的供应商适配器见[Provider 适配器实现指南](docs/01-architecture/17-provider-implementation-guide.md)。

## Cargo features

`ferrin` 门面 crate 的 features：

| feature | 内容 | 默认 |
| --- | --- | --- |
| `macros` | `#[ferrin::tool]` 属性宏 | 是 |
| `openai`、`anthropic`、`google`、`openai-compatible` | 对应的供应商 crate，同时在 `ferrin::openai` 等路径下导出 | 否 |
| `mcp` | MCP 客户端（含 stdio 传输与 OAuth） | 否 |
| `otel` | OpenTelemetry 桥接 `ferrin::otel::OtelTelemetry` | 否 |
| `realtime` | `ferrin-core` 的实时会话循环（WebSocket） | 否 |

各 crate 也可以单独依赖；`ferrin-openai` 的 WebSocket 流式模型（实时转写、语音翻译）需要该 crate 自身的 `realtime` feature。

## 示例

`examples/` 下有七个可运行示例，都读取 `OPENAI_API_KEY`，可用 `OPENAI_BASE_URL` 与 `OPENAI_MODEL`（默认 `gpt-5`）切换端点与模型：

| 示例 | 内容 |
| --- | --- |
| `example-generate-text` | 单步文本生成，打印用量与警告 |
| `example-structured-output` | 让模型填充一个 `Recipe` 结构体 |
| `example-tool-approval` | 工具审批：在终端确认后执行工具 |
| `example-agent` | `ToolLoopAgent` 与 `#[ferrin::tool]` 定义的多个工具 |
| `example-mcp` | 通过 stdio 连接 MCP 服务器并使用其工具（需要 Node.js） |
| `example-stream-sse-server` | hyper 服务器把 `StreamEvent` 以 Server-Sent Events 推送给浏览器 |
| `example-otel` | 导出 GenAI 语义约定的 span 到标准输出 |

```sh
OPENAI_API_KEY=... cargo run -p example-generate-text
OPENAI_API_KEY=... cargo run -p example-stream-sse-server   # then: curl -N 'http://127.0.0.1:3000/chat?prompt=hello'
```

使用工具的示例另外接受 `OPENAI_PROVIDER_OPTIONS`（JSON，按供应商名分组）。部分第三方 OpenAI 兼容代理不支持 Responses API 的 `item_reference`，此时设置 `OPENAI_PROVIDER_OPTIONS='{"openai":{"store":false}}'` 让多步调用回传完整条目。

## 工作区

```text
crates/
  ferrin                   facade: re-exports, prelude, features
  ferrin-core              generation loop, streaming, structured output, agents, middleware, registry, modalities
  ferrin-spec              provider specification: model traits, prompt/content types, stream parts, errors
  ferrin-message           application messages, conversion to provider prompts, pruning
  ferrin-tool              tool definitions, tool sets, approval, repair, sandbox trait
  ferrin-schema            JSON Schema generation (schemars) and dynamic validation
  ferrin-provider-util     HTTP transport, SSE decoding, secure URL policy, settings
  ferrin-macros            #[ferrin::tool]
  ferrin-mcp               MCP client
  ferrin-otel              OpenTelemetry bridge
  ferrin-testing           mock models, fixture server, contract checks
  providers/ferrin-openai, ferrin-anthropic, ferrin-google, ferrin-openai-compatible
examples/                  seven runnable examples
xtask/                     repository tooling (cargo xtask ...)
docs/                      design documents, provider docs, API snapshots
verification/              prototypes behind the pending-verification items (separate workspace)
```

分层规则：`ferrin-spec` 不依赖其他 Ferrin crate；供应商 crate 只依赖 `ferrin-spec` 与 `ferrin-provider-util`；应用只需依赖 `ferrin`。`0.y` 阶段所有 crate 共用一个版本号。

## 项目状态

- 15 个 crate、`xtask` 与七个示例均已实现；全部 crate 为 0.1.0，未发布，未打 tag。
- 测试 665 个（其中 10 个为需要真实凭据的在线测试），CI 在 Linux、macOS、Windows 三平台运行 14 个作业，当前全部通过。
- 真实端点验证：七个示例与全部在线测试在一个第三方 OpenAI 兼容端点上通过。OpenAI 官方端点、Anthropic 与 Google 尚未用真实凭据测试，供应商测试目前基于手工编写的 fixture（待验证事项 PV-031）。
- 设计文档中 31 项待验证事项已关闭 30 项，详见[待验证事项汇总](docs/05-appendix/02-pending-verification.md)。

## 开发

```sh
rustup show           # picks up rust-toolchain.toml (1.98.1)
cargo binstall --locked cargo-nextest cargo-deny cargo-shear cargo-insta \
    cargo-hack cargo-semver-checks cargo-llvm-cov typos-cli just
just check-all        # fmt, clippy, tests, doctests, docs, API snapshot, deny, shear, features, typos, docs lint
```

- 测试只放在 `crates/<crate>/tests/suite/*.rs`，由 `tests/all.rs` 聚合；供应商测试回放 `tests/fixtures/` 下的录制响应，不访问网络。
- 在线测试以 `live_` 开头并标记 `#[ignore]`：`OPENAI_API_KEY=... just test -- --run-ignored only -E 'test(live_)'`。
- `cargo xtask` 提供 `publish-order`、`check-module-size`、`check-versions`、`record-fixture`、`api-snapshot`。公共 API 变化后运行 `cargo xtask api-snapshot` 并提交 `docs/api/`。
- 基准测试：`just bench`（criterion，报告在 `target/criterion/report/index.html`，`just bench sse` 按名称过滤）。覆盖 SSE 解码、部分 JSON 修复、schema、消息裁剪、工具指纹、生成与流式管线、三个供应商适配器（本地 fixture 服务器）和门面端到端流式；不访问网络，结果不入库，也不是 CI 门禁。
- 贡献流程与代码规则见 [CONTRIBUTING.md](CONTRIBUTING.md) 与 [AGENTS.md](AGENTS.md)；每个 crate 的 `CHANGELOG.md` 随改动更新。

## 文档

- [设计文档目录](docs/README.md)：架构、公共 API、工程规范、架构决策记录（ADR）与附录，含各章节的实现记录。
- [API 参考与示例](docs/02-api/02-api-reference.md)：入口函数与构建器的签名和用法。
- rustdoc：`just doc` 生成，文档示例可编译。

## 安全

- API 密钥使用 `secrecy` 类型保存，从环境变量延迟读取，不出现在日志与错误信息中。
- 下载与 MCP 端点遵循安全 URL 策略：仅 HTTPS、拒绝私有网络地址、DNS 固定、大小限制。
- 工具审批签名使用 HMAC-SHA256 与常量时间比较。
- 详细规则见[安全规范](docs/03-engineering/08-security-practices.md)。

## 致谢

Ferrin 的能力范围与核心抽象（Provider 规范、生成循环、工具审批、流式部件、中间件）参考了 [Vercel AI SDK](https://github.com/vercel/ai) 的设计（Apache-2.0）。核心生成循环、供应商适配器与部分算法（如部分 JSON 修复、消息裁剪）由其 TypeScript 实现移植到 Rust 并有修改，涉及的 crate 与模块见 [NOTICE](NOTICE) 及相应的 rustdoc 说明。工程实践（工作区约定、lint 配置、CI 结构）参考了 [OpenAI Codex](https://github.com/openai/codex)。

Ferrin 是独立实现的项目，与 Vercel、OpenAI 均无关联，也不是它们的官方项目。

## 许可

Apache-2.0（[LICENSE](LICENSE)）。派生代码的署名见 [NOTICE](NOTICE)；两份文件同时随每个发布的 crate 分发。

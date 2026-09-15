# API reference and examples

**English** | [Chinese](../zh-CN/02-api/02-api-reference.md)

This document lists facade APIs and typical usage. Signatures describe the target shape; implemented rustdoc is authoritative, and this document must be updated when they differ.

## 1. Providers and models

```rust
use ferrin::openai::{create_openai, OpenAiSettings};
use ferrin::anthropic::{create_anthropic, AnthropicSettings};

let openai = create_openai(OpenAiSettings::default())?;          // reads OPENAI_API_KEY lazily
let anthropic = create_anthropic(AnthropicSettings {
    api_key: Some(SecretString::from(std::env::var("MY_ANTHROPIC_KEY")?)),
    ..Default::default()
})?;

let gpt = openai.responses("gpt-5");
let claude = anthropic.messages("claude-sonnet-4-5");
```

## 2. `generate_text`

```rust
pub fn generate_text(model: impl Into<LanguageModelRef>) -> GenerateText<()>;

impl<O> GenerateText<O> {
    // prompt
    pub fn system(self, instructions: impl Into<Instructions>) -> Self;
    pub fn prompt(self, text: impl Into<String>) -> Self;
    pub fn messages(self, messages: impl IntoIterator<Item = Message>) -> Self;
    pub fn allow_system_in_messages(self) -> Self;

    // sampling
    pub fn max_output_tokens(self, n: u32) -> Self;
    pub fn temperature(self, t: f64) -> Self;
    pub fn top_p(self, p: f64) -> Self;
    pub fn top_k(self, k: u32) -> Self;
    pub fn presence_penalty(self, p: f64) -> Self;
    pub fn frequency_penalty(self, p: f64) -> Self;
    pub fn stop_sequences(self, seqs: impl IntoIterator<Item = impl Into<String>>) -> Self;
    pub fn seed(self, seed: u64) -> Self;
    pub fn reasoning(self, effort: ReasoningEffort) -> Self;

    // tools
    pub fn tools(self, tools: ToolSet) -> Self;
    pub fn tool_choice(self, choice: ToolChoice) -> Self;
    pub fn active_tools(self, names: impl IntoIterator<Item = impl Into<ToolName>>) -> Self;
    pub fn tool_order(self, names: impl IntoIterator<Item = impl Into<ToolName>>) -> Self;
    pub fn tools_context(self, ctx: impl Serialize) -> Self;
    pub fn tool_approval(self, policy: impl ApprovalPolicy + 'static) -> Self;
    pub fn tool_approval_secret(self, secret: SecretBox<[u8]>) -> Self;
    pub fn tool_callers(self, callers: ToolCallers) -> Self;
    pub fn repair_tool_call(self, repair: impl ToolCallRepair + 'static) -> Self;
    pub fn refine_tool_input(self, name: impl Into<ToolName>, f: impl Fn(JsonValue) -> BoxFuture<'static, Result<JsonValue, Error>> + Send + Sync + 'static) -> Self;
    pub fn sandbox(self, sandbox: Arc<dyn Sandbox>) -> Self;

    // loop control
    pub fn stop_when(self, condition: impl StopCondition + 'static) -> Self;      // may be called multiple times (any-of)
    pub fn prepare_step(self, f: impl PrepareStep + 'static) -> Self;
    pub fn output<T>(self, output: Output<T>) -> GenerateText<T>;

    // request
    pub fn max_retries(self, n: u32) -> Self;
    pub fn retry_policy(self, policy: RetryPolicy) -> Self;
    pub fn timeout(self, timeout: impl Into<Timeout>) -> Self;
    pub fn cancellation(self, token: CancellationToken) -> Self;
    pub fn headers(self, headers: Headers) -> Self;
    pub fn provider_options(self, options: ProviderOptions) -> Self;
    pub fn download(self, f: Arc<dyn DownloadFn>) -> Self;
    pub fn include(self, include: Include) -> Self;

    // observability
    pub fn telemetry(self, options: TelemetryOptions) -> Self;
    pub fn on_start(self, f: impl HookFn<StartEvent>) -> Self;
    pub fn on_step_start(self, f: impl HookFn<StepStartEvent>) -> Self;
    pub fn on_language_model_call_start(self, f: impl HookFn<ModelCallStartEvent>) -> Self;
    pub fn on_language_model_call_end(self, f: impl HookFn<ModelCallEndEvent>) -> Self;
    pub fn on_tool_execution_start(self, f: impl HookFn<ToolExecutionStartEvent>) -> Self;
    pub fn on_tool_execution_end(self, f: impl HookFn<ToolExecutionEndEvent>) -> Self;
    pub fn on_step_end(self, f: impl HookFn<StepResult>) -> Self;
    pub fn on_end(self, f: impl HookFn<EndEvent>) -> Self;
}
```

Example: multi-step tool loop.

```rust
use ferrin::prelude::*;

#[derive(Deserialize, JsonSchema)]
struct LookupOrder { order_id: String }

let tools = ToolSet::new().insert(
    "lookup_order",
    Tool::function::<LookupOrder>()
        .description("Look up an order by id.")
        .execute(|input: LookupOrder, _ctx| async move {
            Ok(json!({ "order_id": input.order_id, "status": "shipped" }))
        })
        .build(),
)?;

let result = ferrin::generate_text(&gpt)
    .system("You are a support agent.")
    .prompt("Where is order 4521?")
    .tools(tools)
    .stop_when(step_count(5))
    .on_step_end(|step| async move { tracing::info!(step = step.step_number, "step done") })
    .await?;

println!("{}", result.text());
for step in &result.steps {
    for call in step.tool_calls() {
        println!("called {} with {}", call.tool_name, call.input);
    }
}
```

## 3. `stream_text`

```rust
pub fn stream_text(model: impl Into<LanguageModelRef>) -> StreamText<()>;

impl<O> StreamText<O> {
    // all GenerateText methods, plus:
    pub fn transform(self, t: impl StreamTransform + 'static) -> Self;      // applied in order
    pub fn include_raw_chunks(self) -> Self;
    pub fn stream_retries(self, n: u32) -> Self;
    pub fn on_chunk(self, f: impl HookFn<StreamEvent>) -> Self;
    pub fn on_error(self, f: impl Fn(&Error) -> BoxFuture<'static, ErrorDecision> + Send + Sync + 'static) -> Self;
    pub fn on_abort(self, f: impl HookFn<AbortEvent>) -> Self;
}

impl<O> StreamTextResult<O> {
    pub fn split(self) -> (EventStream, Completion<O>);
    pub fn text_stream(self) -> TextStream;                        // Stream<Item = Result<String, Error>>
    pub fn partial_output_stream(self) -> PartialOutputStream<O>;
    pub fn element_stream(self) -> ElementStream<O::Element> where O: ArrayOutput;
    pub async fn consume(self) -> Result<GenerateTextResult<O>, Error>;
}

impl<O> Completion<O> {
    pub async fn result(self) -> Result<GenerateTextResult<O>, Error>;
}
```

Example: forward SSE events while waiting for the final result.

```rust
let stream = ferrin::stream_text(&claude)
    .prompt("Write a haiku about ownership.")
    .transform(smooth_stream(SmoothStreamConfig::default()))
    .await?;

let (mut events, completion) = stream.split();

let forward = tokio::spawn(async move {
    while let Some(event) = events.next().await {
        let line = serde_json::to_string(&event)?;
        sse_tx.send(line).await?;
    }
    Ok::<_, anyhow::Error>(())
});

let final_result = completion.result().await?;
forward.await??;
println!("total tokens: {:?}", final_result.total_usage.output.total);
```

## 4. Message construction

```rust
let messages = vec![
    Message::system("You are a helpful assistant."),
    Message::user("Describe this image."),
    Message::user_parts([
        UserPart::text("And this document:"),
        UserPart::file_bytes(pdf_bytes, "application/pdf").with_filename("report.pdf"),
        UserPart::image_url("https://example.com/cat.png".parse()?),
    ]),
];

let result = ferrin::generate_text(&gpt).messages(messages).await?;
let mut history = messages;
history.extend(result.response_messages());
```

## 5. Tool approval

```rust
let tools = ToolSet::new().insert(
    "delete_file",
    Tool::function::<DeleteFile>()
        .description("Delete a file.")
        .needs_approval(NeedsApproval::Always)
        .execute(delete_file)
        .build(),
)?;

let first = ferrin::generate_text(&gpt)
    .prompt("Delete temp.log")
    .tools(tools.clone())
    .tool_approval_secret(secret.clone())
    .await?;

let mut history = first.response_messages();
for request in first.last_step().tool_approval_requests() {
    // ask the user; then append the response to the last tool message
    history.push_approval_response(ToolApprovalResponse::approved(request.approval_id.clone()));
}

let second = ferrin::generate_text(&gpt)
    .messages(history)
    .tools(tools)
    .tool_approval_secret(secret)
    .await?;
```

Policy-based approval (feature `policy`, crate `ferrin-policy`; see [Policy-based tool approval](../01-architecture/18-policy-approval.md)):

```rust
use ferrin::policy::{HttpPolicyClient, policy_approval, shadow, Enforcement};

let opa = HttpPolicyClient::builder(url::Url::parse("https://policy.internal.example/")?)
    .header("authorization", "Bearer <token>")
    .build()?;
let policy = shadow(policy_approval(opa, "ferrin/tools/decision"))
    .enforcement(Enforcement::Enforce);

let result = ferrin::generate_text(&gpt)
    .prompt("Delete temp.log")
    .tools(tools)
    .tool_approval(policy)
    .await?;
```

## 6. Structured output

See [Structured output](../01-architecture/08-structured-output.md), section 6.

## 7. Agent

See [Agent](../01-architecture/09-agent.md), section 3.

## 8. Other modalities

```rust
let embeddings = ferrin::embed_many(openai.embedding("text-embedding-3-small"), texts)
    .max_parallel_calls(4)
    .await?;

let image = ferrin::generate_image(openai.image("gpt-image-1"), "A lighthouse at dawn")
    .size(ImageSize::new(1024, 1024))
    .await?;
tokio::fs::write("lighthouse.png", &image.images[0].data).await?;

let speech = ferrin::generate_speech(openai.speech("gpt-4o-mini-tts"), "Hello from Ferrin")
    .voice("alloy")
    .await?;

let transcript = ferrin::transcribe(openai.transcription("gpt-4o-transcribe"), audio /* Bytes or Url */)
    .await?;

let ranked = ferrin::rerank(cohere.reranking("rerank-v3.5"), "rust async", documents)
    .top_n(3)
    .await?;

let upload = ferrin::upload_file(openai.files(), pdf_bytes)
    .media_type("application/pdf")
    .filename("spec.pdf")
    .await?;
let part = UserPart::file_reference(upload.provider_reference, "application/pdf");

// feature `realtime`
let mut session = ferrin::realtime::realtime_session(openai.realtime().model("gpt-realtime")?)
    .instructions("You are a voice assistant.")
    .tools(weather_tools())
    .connect()
    .await?;
let handle = session.handle();
handle.send_text("What is the weather in Rome?").await?;
while let Some(event) = session.next_event().await {
    match event? {
        RealtimeServerEvent::TextDelta { delta, .. } => print!("{delta}"),
        RealtimeServerEvent::ResponseDone { .. } => break,
        _ => {}
    }
}
session.close().await?;
```

(2026-09-13) See [Other modalities](../01-architecture/11-other-modalities.md), section 13, for actual signatures: `embed_many` accepts any IntoIterator of string-convertible items; image generation returns GenerateImageResult with required per-image media_type; transcription accepts `Into<AudioInput>` (`Bytes`, `Vec<u8>`, `Url`); `rerank` has no `usage`; `upload_file` accepts `Into<UploadData>`.

## 9. Middleware and registry

See [Middleware and registry](../01-architecture/10-middleware-and-registry.md), section 3.

## 10. MCP

See [MCP integration](../01-architecture/15-mcp.md), section 4.

## 11. Error handling

```rust
match ferrin::generate_text(&gpt).prompt("hi").await {
    Ok(result) => println!("{}", result.text()),
    Err(err) if err.is_retryable() => tracing::warn!(%err, "transient failure"),
    Err(err) if err.status_code() == Some(StatusCode::UNAUTHORIZED) => {
        eprintln!("check OPENAI_API_KEY");
    }
    Err(Error::NoObjectGenerated(details)) => eprintln!("model returned non-conforming output: {:?}", details.text),
    Err(err) => return Err(err.into()),
}
```

## 12. Testing utilities (ferrin-testing)

```rust
use ferrin_testing::{MockLanguageModel, simulate_stream};

let model = MockLanguageModel::builder()
    .do_generate(|_opts| GenerateResult::text("hello"))
    .do_stream(|_opts| simulate_stream([
        StreamPart::text_start("0"),
        StreamPart::text_delta("0", "hel"),
        StreamPart::text_delta("0", "lo"),
        StreamPart::text_end("0"),
        StreamPart::finish(FinishReasonKind::Stop, Usage::default()),
    ]))
    .build();

let result = ferrin::generate_text(&model).prompt("hi").await?;
assert_eq!(result.text(), "hello");
```

`MockLanguageModel` records `CallOptions` snapshots through `model.calls()` for request assertions.

## 13. Stability annotations

| Module | Stability during `0.y` |
| --- | --- |
| Text generation/streaming, messages, tools, structured output, errors | Core; changes require an ADR |
| Agents, middleware, registry, telemetry | Core |
| Embeddings, images, speech, transcription, reranking, file upload | Core |
| Video, batches, realtime, speech translation, streaming transcription | Provider APIs still evolve; documented as Stability: evolving, with minor-version changes allowed |
| `ferrin-mcp` | evolving |
| `ferrin-policy` | evolving |
| `Sandbox` | evolving |

## 14. Implementation record (2026-09-14, ferrin facade)

- [Fact] The root re-exports all public core modules (`agent`, `batch`, `embed`, `generate_text`, `stream_text`, `output`, `middleware`, `registry`, `telemetry`, `retry`, `timeout`, modalities) and entry points/types including generation, embeddings, images, speech, transcription, reranking, uploads, `step_count`, `has_tool_call`, `Error`, `Output`, `Agent`, `ToolLoopAgent`, and `TelemetryOptions`. Realtime module/session are feature-gated. Lower crates are aliased as spec/message/schema/tool/provider_util. Re-export `serde`, `serde_json`, and `schemars` for derive crate attributes and `json!`.
- [Fact] Providers are aliased both at root (openai, anthropic, google, openai_compatible) and under providers, consistent with [Crate boundaries](../01-architecture/02-crates.md), section 5. MCP/OTel use matching feature gates. The default-enabled `macros` feature exports `#[ferrin::tool]`, coexisting with the tool module in separate namespaces.
- [Decision] Prelude includes entry points/results, `Message`/`UserPart`/`AssistantPart`/`MessagesExt`/`ToolApprovalResponse`/`Role`, `Tool`/`ToolSet`/`ToolContext`/`ToolError`/`NeedsApproval`/`Schema`/`JsonSchema`, common specification model traits/references, `ToolChoice`, `ReasoningEffort`, FinishReason/Kind, `Usage`, JSON aliases, `Headers`, `ProviderOptions`, `ImageSize`, `ProviderError`, `StreamPart`, `GenerateResult`, `Content`, `serde` derives, json!, StreamExt, and the `tool` macro. This compiles section 2 with one prelude import. Derives still require direct `serde`/`schemars` dependencies or explicit ferrin crate-path attributes, as documented in rustdoc/README.
- [Fact] Facade suite tests cover prelude generation/streaming/tool loops using mocks, feature re-exports, macro expansion (async/sync/context/docs/field schema descriptions), and `trybuild` (one pass, seven failures: references, nested references, lifetimes, generics, no input, no return, macro arguments). Initial `trybuild` compiles a separate project; nextest gives it a 180 s slow-test period.

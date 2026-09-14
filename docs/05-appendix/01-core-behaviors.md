# Core behavior checklist

**English** | [Chinese](../zh-CN/05-appendix/01-core-behaviors.md)

This checklist groups core behavior by capability for implementation/review. Each row corresponds to an architecture fact or decision; Ferrin identifies types/functions/settings/tradeoffs, while notes explain scope/rationale. Referenced chapters contain authoritative details.

## 1. Prompts and messages

Chapters: [Prompt conversion](../01-architecture/05-prompt-conversion.md), [Data model](../01-architecture/03-core-data-model.md).

| Behavior | Ferrin | Notes |
| --- | --- | --- |
| Exclusive `prompt`/`messages` | `Error::InvalidPrompt` | |
| Prepend `system`; reject embedded systems by default | Explicit allow_system_in_messages | |
| Download and inline unsupported URLs | `DefaultDownloader` | Secure policy, configurable concurrency |
| Normalize images to files; detect magic bytes | `detect_media_type` | Maintained signature table |
| Strip approval responses before model input | Prompt conversion | Core replay only |
| Normalize text/JSON/errors | `create_tool_model_output` | |
| Pass references to adapters | Four-state `FileData` | Missing key: `NoSuchProviderReference` |
| Prune reasoning/tool calls/empty messages | `ferrin_message::prune` | |

## 2. Generation loop

Chapter: [Generation and streaming](../01-architecture/07-generation-loop-and-streaming.md).

| Behavior | Ferrin | Notes |
| --- | --- | --- |
| Continue when all client calls have output/denial, calls or deferred results remain, and no stop condition holds | `should_continue` | |
| Default one step; agents twenty | `step_count(1)`/`step_count(20)` | |
| Override model/choice/active tools/messages/context per step | `prepare_step` | |
| Execute tools only for `stop`/`tool-calls` finish | Loop checks | |
| Track deferred provider results | Step state | |
| Parse `output` for `stop` or non-tool-call with text | `Output` | Otherwise `NoOutputGenerated`; generic `output` |
| Two retries, 2 s start, factor two, retry headers 0–60 s | `RetryPolicy` | Optional jitter |
| Total/step/first/chunk/tool/per-tool timeouts | `Timeouts`, `Duration` | |
| Body/message/raw recording defaults off | `Include` | |
| Assemble response messages | `to_response_messages` | |
| Per-step metrics | `StepPerformance` | |
| Separate object-generation API | None | generate_text with output only |

## 3. Tools

Chapter: [Tool system](../01-architecture/06-tool-system.md).

| Behavior | Ferrin | Notes |
| --- | --- | --- |
| Four tool kinds | `ToolKind` | |
| Single or preliminary/final streamed output | `ToolOutput` stream | |
| Parse, repair, then mark invalid instead of throwing | `ParsedToolCall::invalid` | |
| Invalid tool-choice violations | Same | |
| Input refinement | `refine_tool_input` | |
| Four approval states and precedence | `ApprovalPolicy`/`ApprovalStatus` | |
| HMAC-SHA256 signatures | `ferrin-tool-approval-v1` domain | Constant-time comparison |
| Revalidate input/policy on replay | `validate_tool_approvals` | |
| Fingerprints/drift | `fingerprint_tools`/`detect_tool_drift` | |
| Caller restrictions | `ToolCallers` | |
| Active tools/order | `active_tools`/`tool_order` | |
| Context schemas | JSON plus validation | |
| `Sandbox` session | `Sandbox` trait | Local testing implementation only |
| Typed results | Typed definitions, JSON plus extraction | [ADR 0012](../04-decisions/2026-09-13-0012-tool-typing-strategy.md) |

## 4. Streaming

Chapter: [Generation and streaming](../01-architecture/07-generation-loop-and-streaming.md).

| Behavior | Ferrin | Notes |
| --- | --- | --- |
| Execution/stitching/resilience/stop/transforms/output/processing | Pipeline stages | |
| Event inventory | Serializable `StreamEvent` | |
| Multiple views | One stream plus `Completion` | [ADR 0005](../04-decisions/2026-09-13-0005-stream-result-delivery.md) |
| Startup | `Result` after first request establishment | Configuration errors through ? |
| Stream retries/error hook | `stream_retries`/`on_error` | Off by default |
| Remap part IDs | Multi-step streams | |
| Word/line/regex/segmenter/detector smoothing | `smooth_stream` | `unicode-segmentation` |
| Frontend message protocol | None | Events are serializable |

## 5. Specification and adapters

Chapters: [Specification](../01-architecture/04-provider-spec.md), [Provider guide](../01-architecture/17-provider-implementation-guide.md).

| Behavior | Ferrin | Notes |
| --- | --- | --- |
| Specification version | Crate version | No runtime coexistence; [ADR 0011](../04-decisions/2026-09-13-0011-spec-versioning-by-crate-version.md) |
| Twelve interface families | Specification traits | Optional methods use defaults/capability queries |
| Warn for unsupported options | `Warning::Unsupported` | Adapter contract 1 |
| Reasoning effort/budget mapping | `map_reasoning_to_effort`/`map_reasoning_to_budget` | |
| Lazy credentials | Request construction | Factories perform no I/O |
| Provider tool namespaces | `openai::tools` and equivalents | |
| Workflow serialization hooks | None | |

## 6. Middleware, registry, telemetry

Chapters: [Middleware and registry](../01-architecture/10-middleware-and-registry.md), [Observability](../01-architecture/13-observability.md).

| Behavior | Ferrin | Notes |
| --- | --- | --- |
| Six hooks, reverse wrapping | `LanguageModelMiddleware`/`wrap_language_model` | |
| Five baseline built-ins | `middleware` | |
| Provider:model resolution/errors | `ProviderRegistry` | |
| Custom provider/fallback | `custom_provider` | |
| Implicit global provider | None | [ADR 0008](../04-decisions/2026-09-13-0008-no-implicit-default-provider.md) |
| `Telemetry` hooks/options | `Telemetry`/`TelemetryOptions` | Synchronous hooks |
| Global telemetry registry/diagnostic channel | None | Tracing instead |
| Global warning switch | None | Target filtering |

## 7. HTTP and security

Chapter: [HTTP security](../01-architecture/14-http-and-security.md).

| Behavior | Ferrin | Notes |
| --- | --- | --- |
| Injectable transport | `HttpTransport` | |
| Response `handlers` | `handlers` | |
| Retryable 408/409/429/5xx | `ApiCallError::is_retryable` | |
| Secure URL rules | `secure_url` | Clippy enforcement |
| Prototype pollution defense | Unnecessary | Rust maps; resource limits instead |
| Prefixed random IDs | `IdGenerator` | |
| User-Agent chain | `ferrin/<version>` then `ferrin-<provider>/<version>` | |

## 8. Other modalities

Chapter: [Other modalities](../01-architecture/11-other-modalities.md).

| Behavior | Ferrin | Notes |
| --- | --- | --- |
| Chunked parallel embeddings | `embed_many` | UTF-8 byte counts, PV-011 |
| Multiple image calls, empty retries | `generate_image` | |
| Speech/transcription/reranking | `generate_speech`/`transcribe`/`rerank` | |
| Video polling/webhook/fallback | `generate_video` | Application webhook factory |
| File/skill uploads | `upload_file`/`upload_skill` | |
| Five `batch` functions | `batch` | |
| Realtime sessions | `realtime` feature | No browser transport |
| Streaming translation | `SpeechTranslationModel` | |

## 9. MCP

Chapter: [MCP](../01-architecture/15-mcp.md).

| Behavior | Ferrin | Notes |
| --- | --- | --- |
| Config/custom transport | `TransportConfig`/`McpTransport` | Event stream replaces callbacks |
| Discovery/negotiation | Dual-generation client | |
| Dynamic/typed tool bridge | `client.tools(ToolsOptions)` | Automatic server schemas use strict false |
| Tool retry classification | `McpError::is_retryable_tool_call` | |
| Resources/prompts/completion/elicitation/Apps/headers/OAuth | Corresponding methods/features | |
| Stdio | `stdio` feature | Common Rust application use |

## 10. Engineering practices

See [Engineering standards](../03-engineering/) for layout, coding, testing, CI, releases, ADRs, security, and documentation; these are not repeated here.

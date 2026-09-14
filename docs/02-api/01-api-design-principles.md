# API design principles

**English** | [Chinese](../zh-CN/02-api/01-api-design-principles.md)

These rules govern the public API of the `ferrin` facade and `ferrin-core`.

## 1. Invocation shape

[Decision] Entry points return builders implementing `IntoFuture`:

```rust
pub fn generate_text(model: impl Into<LanguageModelRef>) -> GenerateText<()>;
pub fn stream_text(model: impl Into<LanguageModelRef>) -> StreamText<()>;
pub fn embed(model: impl Into<EmbeddingModelRef>, value: impl Into<String>) -> Embed;
// ...

impl<O: Send + 'static> IntoFuture for GenerateText<O> {
    type Output = Result<GenerateTextResult<O>, Error>;
    type IntoFuture = BoxFuture<'static, Self::Output>;
}
```

Rationale:

- Calls have dozens of optional sampling, tool, stop, hook, and telemetry settings. Positional parameters or one configuration object are hard to read and extend; builders are idiomatic Rust.
- `IntoFuture` permits direct awaiting without send/run terminators, using a request-builder style of composition.
- Adding builder methods preserves compatibility and conservative API evolution.

Builders are Send and static, allowing transfer between tasks after construction.

## 2. Parameter types

| Situation | Form | Example |
| --- | --- | --- |
| Mode selection | Enum | `ToolChoice::Required`, `Chunking::Line` |
| Switch | Named method without a boolean | `.allow_system_in_messages()` rather than `.system_in_messages(true)` |
| Optional value | Calling a method sets it; omission uses defaults | `.temperature(0.2)` |
| Collection | `impl IntoIterator<Item = impl Into<T>>` | `.stop_sequences(["END"])` |
| Callback | `impl Fn(...) -> Fut + Send + Sync + 'static` | `.on_step_end(|step| async move { ... })` |
| Model | `impl Into<LanguageModelRef>`, accepting borrowed/shared Arc, concrete models, or strings with an explicit default registry | `generate_text(&model)` |
| Duration | `std::time::Duration` | `.timeout(Duration::from_secs(30))` |
| Binary | `bytes::Bytes` or `impl Into<Bytes>` | `UserPart::image_bytes(data)` |

[Decision] Avoid boolean or bare `Option` positional parameters. Calls such as `foo(false)` and `bar(None)` hide meaning; enums and named methods explain themselves and permit compatible expansion.

## 3. Result types

- Result fields are public, with `Debug`/`Clone`; conveniences such as `text()` and `tool_calls()` are methods.
- Structured output uses generic O, defaulting to `()`.
- Unify application errors as `ferrin::Error`; expose no third-party error types other than boxed standard Error trait objects.

## 4. Type stability

- Mark public enums/errors non_exhaustive.
- Mark structs expected to gain fields non_exhaustive and provide constructors/builders.
- Allowed third-party public types: serde_json Value/Map, bytes Bytes, url Url, http HeaderMap/StatusCode/Method, chrono `DateTime<Utc>`, Tokio CancellationToken, futures_core Stream, schemars JsonSchema bounds, and secrecy SecretString. Major upgrades of these crates are Ferrin breaking changes.

## 5. Naming

- Functions/methods use snake_case verb phrases, such as `generate_text` and `wrap_language_model`.
- Types use established domain nouns such as `StepResult`, `StopCondition`, and `ToolSet`, without version/stability affixes.
- Avoid abbreviations; use `Id` suffixes for identifiers.
- Prefix provider types with Rust-style provider names: `OpenAiProvider` and `AnthropicSettings`, using `OpenAi` rather than `OpenAI`.

## 6. Documentation requirements

- Document every public item, purpose, defaults, and errors; examples must compile as doctests.
- Entry points include a minimal example and a tool example.
- Evolving provider beta/preview capabilities or unsettled Ferrin interfaces use a Stability section explaining possible changes during `0.y`, without `experimental_` prefixes.

## 7. Facade structure

```rust
// ferrin/src/lib.rs
pub use ferrin_core::*;
pub mod spec { pub use ferrin_spec::*; }
pub mod schema { pub use ferrin_schema::*; }
pub mod prelude {
    pub use ferrin_core::{generate_text, stream_text, embed, embed_many, step_count, has_tool_call, Output, ToolSet, Tool, Message, Error};
    pub use ferrin_spec::{Warning, Usage, FinishReason};
    pub use futures_util::StreamExt as _;
}
#[cfg(feature = "openai")] pub mod openai { pub use ferrin_openai::*; }
#[cfg(feature = "anthropic")] pub mod anthropic { pub use ferrin_anthropic::*; }
#[cfg(feature = "mcp")] pub mod mcp { pub use ferrin_mcp::*; }
#[cfg(feature = "otel")] pub mod otel { pub use ferrin_otel::*; }
#[cfg(feature = "macros")] pub use ferrin_macros::tool;
```

## 8. Key tradeoffs

| Common approach | Ferrin | Rationale |
| --- | --- | --- |
| One configuration object | Builder | Rust idiom; extensible |
| Global default provider for string IDs | Explicit default registry | No implicit networking |
| Synchronous stream handle with configuration errors in-stream | Await first request establishment | Handle configuration errors with ? at the call site |
| Multiple tee views | One event stream plus `Completion` | Backpressure and ownership |
| Experimental prefixes and alias/deprecation chains | None | No legacy compatibility burden |
| Runtime optional output format | Generic O | Compile-time guarantees |
| Tool-set generics infer result types | Typed definitions, JSON results, extraction helpers | Rust generic complexity |
| Separate object generation | generate_text with output | One path |

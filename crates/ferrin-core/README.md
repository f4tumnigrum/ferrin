# ferrin-core

Ferrin core: text generation loop, streaming pipeline, structured output,
agents, middleware, provider registry, retry and timeout policies, and the
non-text modalities (embeddings, images, speech, transcription, reranking,
video, files, skills, batches, realtime sessions, speech translation).

Part of the [Ferrin](https://github.com/f4tumnigrum/ferrin) workspace. Applications normally depend
on the `ferrin` facade crate, which re-exports this crate together with the
provider adapters. Design: `docs/01-architecture/07-generation-loop-and-streaming.md`
and the following architecture documents.

## Entry points

| Function | Purpose |
| --- | --- |
| `generate_text(model)` / `stream_text(model)` | Multi-step tool loop, non-streaming and streaming |
| `Output::{text, object, array, choice, json}` | Structured output strategies for both entry points |
| `ToolLoopAgent::builder(model)` | Reusable agent configuration implementing the `Agent` trait |
| `wrap_language_model`, `middleware::builtin::*` | Language model middleware |
| `create_provider_registry`, `custom_provider` | `provider:model` string resolution |
| `embed`, `embed_many`, `cosine_similarity` | Embeddings |
| `generate_image`, `image::edit_image` | Image generation and editing |
| `generate_speech`, `transcribe`, `stream_transcribe` | Speech synthesis and transcription |
| `rerank` | Document reranking |
| `generate_video` (feature `video`, default on) | Video generation with polling or webhooks |
| `upload_file`, `files::*`, `upload_skill` | Provider file and skill storage |
| `start_batch`, `get_batch_status`, `get_batch_results`, `cancel_batch`, `list_batches` | Batch processing |
| `realtime::realtime_session` (feature `realtime`) | WebSocket realtime sessions with local tool execution |
| `stream_speech_translation` | Streaming speech translation |

Every entry point returns a builder that implements `IntoFuture`; call
`.await` to run it. Builders accept a retry policy, a cancellation token,
timeouts, extra headers and provider options.

## Example

```rust,no_run
use ferrin_core::generate_text;
use ferrin_spec::LanguageModelRef;

async fn run(model: LanguageModelRef) -> Result<(), ferrin_core::Error> {
    let result = generate_text(model)
        .system("You are a concise assistant.")
        .prompt("Explain what a Rust lifetime is in one sentence.")
        .await?;
    let _ = result.text();
    Ok(())
}
```

Provider crates (`ferrin-openai`, `ferrin-anthropic`, ...) supply the model
references; `ferrin-testing` provides `MockLanguageModel` for tests.

## Features

| Feature | Default | Effect |
| --- | --- | --- |
| `video` | on | `generate_video` and the `video` module |
| `realtime` | off | `realtime` module (adds `tokio-tungstenite` with rustls and the system root store) |
| `sandbox` | off | Sandbox plumbing for tool execution (`ferrin-tool/sandbox`) |

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). Portions of this crate are derived from the Vercel AI SDK (Apache-2.0); the crate and module documentation carry the attribution.

# ferrin-openai

Ferrin provider for OpenAI: Responses (default language model), Chat
Completions, legacy Completions, embeddings, images, speech, transcription,
files, skills, batch processing and realtime sessions. The `realtime` feature
adds the WebSocket-backed streaming transcription and speech translation
models.

Part of the [Ferrin](../../../README.md) workspace. Design:
`docs/01-architecture/17-provider-implementation-guide.md`; capability matrix,
provider options and metadata: `docs/providers/openai.md`.

```rust,no_run
use ferrin_openai::OpenAiSettings;
use ferrin_openai::create_openai;

# fn main() -> Result<(), ferrin_spec::error::ProviderError> {
// Reads OPENAI_API_KEY on the first request.
let provider = create_openai(OpenAiSettings::default())?;
let model = provider.responses("gpt-5");
# let _ = model;
# Ok(())
# }
```

## Features

- `realtime`: enables `OpenAiSpeechTranslationModel` and streaming
  transcription over WebSocket (`tokio-tungstenite`).

## Testing

Tests live in `tests/suite/` and replay hand-authored fixtures from
`tests/fixtures/` through `ferrin_testing::FixtureServer`; no network access
or API key is needed:

```sh
cargo nextest run -p ferrin-openai --all-features
```

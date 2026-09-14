# ferrin-google

Ferrin provider for Google Generative AI (Gemini): the `generateContent`
language model (generate and stream, thinking, structured output, grounding
sources, code execution, provider-executed tools), embeddings, Gemini image
generation, text-to-speech, transcription through the Interactions API, Veo
video operations, the Files API, batch generation and Live API sessions.

Part of the [Ferrin](../../../README.md) workspace. Design:
`docs/01-architecture/17-provider-implementation-guide.md`; capability
matrix, provider options and metadata: `docs/providers/google.md`.

```rust,no_run
use ferrin_google::GoogleSettings;
use ferrin_google::create_google;

# fn main() -> Result<(), ferrin_spec::error::ProviderError> {
// Reads GOOGLE_GENERATIVE_AI_API_KEY on the first request.
let provider = create_google(GoogleSettings::default())?;
let model = provider.language_model("gemini-2.5-flash");
# let _ = model;
# Ok(())
# }
```

## Testing

Tests live in `tests/suite/` and replay hand-authored fixtures from
`tests/fixtures/` through `ferrin_testing::FixtureServer`; no network access
or API key is needed:

```sh
cargo nextest run -p ferrin-google
```

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). Portions of this crate are derived from the Vercel AI SDK (Apache-2.0); the crate and module documentation carry the attribution.

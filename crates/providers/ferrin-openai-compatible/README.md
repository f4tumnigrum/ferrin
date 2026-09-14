# ferrin-openai-compatible

Ferrin provider for OpenAI-compatible endpoints: a configurable adapter for
services that expose OpenAI-shaped Chat Completions, Completions, embeddings
and image endpoints. Use it directly against any endpoint, or as the building
block of a dedicated provider crate that supplies its own name, error body
structure, metadata extractor and request body transformer.

Part of the [Ferrin](https://github.com/f4tumnigrum/ferrin) workspace. Design:
`docs/01-architecture/02-crates.md`; capability matrix, settings and provider
options: `docs/providers/openai-compatible.md`.

## Features

- `create_openai_compatible(OpenAiCompatibleSettings)` with `name`,
  `base_url`, optional `api_key` / `api_key_env`, extra headers and query
  parameters, `include_usage`, `supports_structured_outputs`,
  `supported_urls`, embedding limits and hooks (`ErrorStructure`,
  `MetadataExtractor`, `transform_request_body`, `convert_usage`).
- Provider ids `<name>.chat`, `<name>.completion`, `<name>.embedding`,
  `<name>.image`; provider options under `openaiCompatible`, `<name>` and
  its camelCase variant, unknown keys passed through to the request body.
- Chat Completions (generate and stream): tools and tool choice, structured
  outputs, reasoning content, thought signatures, usage details.
- Legacy Completions (generate and stream), embeddings, image generation and
  multipart image edits.

## Example

```rust,no_run
use ferrin_openai_compatible::OpenAiCompatibleSettings;
use ferrin_openai_compatible::create_openai_compatible;
use ferrin_spec::LanguageModel;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::PromptMessage;
use url::Url;

# async fn run() -> Result<(), ferrin_spec::error::ProviderError> {
let mut settings = OpenAiCompatibleSettings::new(
    "example",
    Url::parse("https://api.example.com/v1")?,
);
settings.api_key_env = Some("EXAMPLE_API_KEY".to_owned());
let provider = create_openai_compatible(settings)?;
let result = provider
    .chat("example-model")
    .do_generate(CallOptions::new(vec![PromptMessage::user_text("Hello")]))
    .await?;
println!("{:?}", result.content);
# Ok(())
# }
```

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). Portions of this crate are derived from the Vercel AI SDK (Apache-2.0); the crate and module documentation carry the attribution.

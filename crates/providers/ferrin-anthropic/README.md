# ferrin-anthropic

Ferrin provider for Anthropic: the Messages API language model (generate and
stream, extended thinking, structured output, citations, prompt caching,
MCP connectors, containers and skills), provider-defined and
provider-executed tools, file and skill uploads, and the Message Batches
API.

Part of the [Ferrin](https://github.com/f4tumnigrum/ferrin) workspace. Design:
`docs/01-architecture/17-provider-implementation-guide.md`; capability
matrix, provider options and metadata: `docs/providers/anthropic.md`.

```rust,no_run
use ferrin_anthropic::AnthropicSettings;
use ferrin_anthropic::create_anthropic;

# fn main() -> Result<(), ferrin_spec::error::ProviderError> {
// Reads ANTHROPIC_API_KEY (or ANTHROPIC_AUTH_TOKEN) on the first request.
let provider = create_anthropic(AnthropicSettings::default())?;
let model = provider.messages("claude-sonnet-4-5");
# let _ = model;
# Ok(())
# }
```

## Testing

Tests live in `tests/suite/` and replay hand-authored fixtures from
`tests/fixtures/` through `ferrin_testing::FixtureServer`; no network access
or API key is needed:

```sh
cargo nextest run -p ferrin-anthropic
```

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). Portions of this crate are derived from the Vercel AI SDK (Apache-2.0); the crate and module documentation carry the attribution.

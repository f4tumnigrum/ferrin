# ferrin

Ferrin: a Rust AI SDK for text generation, streaming, tools with approval,
agents, structured output, the other modalities (embeddings, images, speech,
transcription, reranking, video), an MCP client and OpenTelemetry export.

This crate is the facade: it re-exports the `ferrin-core` API at the crate
root, the lower layers as modules (`spec`, `message`, `schema`, `tool`,
`provider_util`), and behind features the provider crates
(`ferrin::openai`, `ferrin::anthropic`, `ferrin::google`,
`ferrin::openai_compatible`, `ferrin::azure`, `ferrin::voyage`, also grouped under `ferrin::providers`), the MCP
client (`ferrin::mcp`), the OpenTelemetry bridge (`ferrin::otel`),
policy-based tool approval (`ferrin::policy`) and the `#[ferrin::tool]`
macro. `ferrin::prelude` gathers the items most programs
need.

Part of the [Ferrin](https://github.com/f4tumnigrum/ferrin) workspace. Design:
`docs/01-architecture/02-crates.md`, API: `docs/02-api/02-api-reference.md`.

## Example

```rust
use ferrin::prelude::*;
use ferrin::openai::{create_openai, OpenAiSettings};

#[derive(Deserialize, JsonSchema)]
#[serde(crate = "ferrin::serde")]
#[schemars(crate = "ferrin::schemars")]
struct LookupOrder {
    order_id: String,
}

/// Look up an order by id.
#[ferrin::tool]
async fn lookup_order(input: LookupOrder) -> Result<JsonValue, ToolError> {
    Ok(json!({ "order_id": input.order_id, "status": "shipped" }))
}

async fn run() -> Result<(), ferrin::Error> {
    let openai = create_openai(OpenAiSettings::default())?; // reads OPENAI_API_KEY lazily
    let result = generate_text(openai.responses("gpt-5"))
        .system("You are a support agent.")
        .prompt("Where is order 4521?")
        .tools(ToolSet::new().insert("lookup_order", lookup_order())?)
        .stop_when(step_count(5))
        .await?;
    println!("{}", result.text());
    Ok(())
}
```

## Features

| Feature | Default | Effect |
|---|---|---|
| `macros` | on | `#[ferrin::tool]` (crate `ferrin-macros`) |
| `openai` | off | `ferrin::openai` (crate `ferrin-openai`) |
| `anthropic` | off | `ferrin::anthropic` (crate `ferrin-anthropic`) |
| `google` | off | `ferrin::google` (crate `ferrin-google`) |
| `azure` | off | `ferrin::azure` (crate `ferrin-azure`) |
| `voyage` | off | `ferrin::voyage` (crate `ferrin-voyage`) |
| `openai-compatible` | off | `ferrin::openai_compatible` (crate `ferrin-openai-compatible`) |
| `mcp` | off | `ferrin::mcp` (crate `ferrin-mcp`) |
| `otel` | off | `ferrin::otel` (crate `ferrin-otel`) |
| `policy` | off | `ferrin::policy` (crate `ferrin-policy`) |
| `policy-rego` | off | `policy` plus the embedded Rego engine (`ferrin-policy/rego`) |
| `realtime` | off | `ferrin::realtime` plus WebSocket transcription/translation on enabled OpenAI/Google providers |

## Testing

`cargo nextest run -p ferrin --all-features` runs the prelude, provider and
macro tests plus the `trybuild` compile-fail cases in `tests/ui/`
(`TRYBUILD=overwrite` refreshes the `.stderr` snapshots after a compiler or
macro change).

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE).

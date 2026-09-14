# ferrin-tool

Tool definitions for Ferrin: `Tool`, `ToolKind`, `ToolSet`, the execution
contract (`ToolExecute`, `ToolOutput`, `ToolContext`, `ToolError`), approval
declarations (`NeedsApproval`), model-output normalisation, caller
restrictions, tool fingerprints and (feature `sandbox`) the `Sandbox` trait
with a local-process implementation.

Part of the [Ferrin](../../README.md) workspace. Design:
`docs/01-architecture/06-tool-system.md`, ADR 0012.

## Example

```rust
use ferrin_tool::Tool;
use ferrin_tool::ToolSet;
use ferrin_tool::ToolError;

#[derive(serde::Deserialize, schemars::JsonSchema)]
struct GetWeather {
    /// City name.
    city: String,
}

#[derive(serde::Serialize)]
struct Weather { temperature_c: f32 }

fn tools() -> Result<ToolSet, ferrin_tool::DuplicateToolError> {
    let get_weather = Tool::function::<GetWeather>()
        .description("Get the current weather for a city.")
        .execute(|input: GetWeather, _ctx| async move {
            let _ = input.city;
            Ok::<_, ToolError>(Weather { temperature_c: 21.5 })
        })
        .build();
    ToolSet::new().insert("get_weather", get_weather)
}
```

Tool call parsing, approval resolution, scheduling and repair live in
`ferrin-core`; this crate describes tools and runs single executions.

## Features

| Feature | Default | Effect |
|---|---|---|
| `sandbox` | off | `Sandbox`/`SandboxProcess` traits, `LocalProcessSandbox`, `ToolContext::sandbox` |

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). Portions of this crate are derived from the Vercel AI SDK (Apache-2.0); the crate and module documentation carry the attribution.

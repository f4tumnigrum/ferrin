# ferrin-testing

Ferrin testing support: mock language models, stream simulation, a fixture
server for provider tests, specification contract checks and a recording HTTP
transport.

Part of the [Ferrin](../../README.md) workspace. Design:
`docs/03-engineering/04-testing.md`. Intended for tests of applications and
provider crates; not for production use.

## Contents

| Item | Purpose |
| --- | --- |
| `MockLanguageModel`, `MockLanguageModelBuilder` | Scripted `do_generate` / `do_stream` results; records every `CallOptions` |
| `simulate_stream`, `SimulatedStream`, `text_parts` | Build specification stream results from parts, with optional delays |
| `StreamContractChecker`, `ContractViolation` | Assert the event ordering contract of a provider stream |
| `FixtureServer`, `Fixture`, `FixtureBody`, `ReceivedRequest` | Local HTTP server replaying JSON and SSE fixtures (single hyper 1.x backend, per-chunk delays) |
| `RecordingTransport`, `RecordedRequest`, `RecordedResponse`, `HeaderFilter`, `redact_secrets` | Capture requests and responses for fixture recording with secrets stripped |
| `SequentialIdGenerator` | Deterministic ids for snapshots |
| `api_call_error` | Shortcut for an HTTP-status provider error |

## Example

```rust,no_run
use ferrin_spec::GenerateResult;
use ferrin_testing::MockLanguageModel;

let model = MockLanguageModel::builder()
    .generate(GenerateResult::text("hello"))
    .build();
let _ = model;
```

## License

MIT OR Apache-2.0.

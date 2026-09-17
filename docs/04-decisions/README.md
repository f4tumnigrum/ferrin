# Architecture decision records

**English** | [Chinese](../zh-CN/04-decisions/README.md)

See the [ADR process](../03-engineering/07-adr-process.md).

| Number | Title | Status | Date |
| --- | --- | --- | --- |
| [0001](2026-09-13-0001-workspace-and-crate-boundaries.md) | Workspace and crate boundaries | accepted | 2026-09-13 |
| [0002](2026-09-13-0002-async-trait-shape-and-dynamic-dispatch.md) | Async traits and dynamic dispatch | accepted | 2026-09-13 |
| [0003](2026-09-13-0003-json-value-and-serialization.md) | JSON values and serialization | accepted | 2026-09-13 |
| [0004](2026-09-13-0004-schema-library-and-dialect.md) | Schema library and dialect | accepted | 2026-09-13 |
| [0005](2026-09-13-0005-stream-result-delivery.md) | Streaming result delivery | accepted | 2026-09-13 |
| [0006](2026-09-13-0006-error-model.md) | Error model | accepted | 2026-09-13 |
| [0007](2026-09-13-0007-tokio-runtime-and-cancellation.md) | Tokio and cancellation | accepted | 2026-09-13 |
| [0008](2026-09-13-0008-no-implicit-default-provider.md) | No implicit default provider | accepted | 2026-09-13 |
| [0009](2026-09-13-0009-http-transport-and-secure-url.md) | HTTP transport and secure URLs | accepted | 2026-09-13 |
| [0010](2026-09-13-0010-mcp-protocol-implementation.md) | MCP protocol implementation | accepted | 2026-09-13 |
| [0011](2026-09-13-0011-spec-versioning-by-crate-version.md) | Specification versioning through crate versions | accepted | 2026-09-13 |
| [0012](2026-09-13-0012-tool-typing-strategy.md) | Tool typing strategy | accepted | 2026-09-13 |
| [0013](2026-09-13-0013-core-implementation-revisions.md) | Core implementation revisions | accepted | 2026-09-13 |
| [0014](2026-09-13-0014-openai-compatible-model-families.md) | Compatible model families and Responses | accepted | 2026-09-13 |
| [0015](2026-09-14-0015-mcp-stdio-frame-writer.md) | Serialize MCP stdio frames through a writer task | accepted | 2026-09-14 |
| [0016](2026-09-14-0016-inline-encoding-no-spawn-blocking.md) | Inline encoding without `spawn_blocking` | accepted | 2026-09-14 |
| [0017](2026-09-14-0017-apache-2-license-and-attribution.md) | Apache-2.0 licensing and attribution | accepted | 2026-09-14 |
| [0018](2026-09-14-0018-english-primary-documentation.md) | English as the primary documentation edition | accepted | 2026-09-14 |
| [0019](2026-09-15-0019-fallible-schema-transforms.md) | Fallible provider schema transforms | accepted | 2026-09-15 |
| [0020](2026-09-15-0020-policy-based-tool-approval.md) | Policy-based tool approval crate and embedded Rego engine | accepted | 2026-09-15 |
| [0021](2026-09-17-0021-agent-runtime-context.md) | Agent runtime context and persistent step state | proposed | 2026-09-17 |
| [0022](2026-09-17-0022-provider-tool-roundtrips.md) | Provider tool roundtrips | proposed | 2026-09-17 |
| [0023](2026-09-17-0023-google-interactions-and-live-audio.md) | Google Interactions and Live audio | proposed | 2026-09-17 |
| [0025](2026-09-17-0025-azure-and-voyage-providers.md) | Azure OpenAI and Voyage providers | proposed | 2026-09-17 |
| [0026](2026-09-17-0026-reference-sdk-parity.md) | Behavioral parity with the reference SDK | proposed | 2026-09-17 |

## Editorial revisions

- 2026-09-14: edited ADR 0001–0014 context/rationale/alternatives/consequences to replace external-project references with technical facts and Ferrin constraints. Decisions, statuses, and dates were unchanged.

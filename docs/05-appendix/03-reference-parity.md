# Module parity review

**English** | [Chinese](../zh-CN/05-appendix/03-reference-parity.md)

[Decision] This review follows [ADR 0026](../04-decisions/2026-09-17-0026-reference-sdk-parity.md) and covers existing Ferrin modules only. The reference is the local Vercel AI SDK checkout at commit `6c6c221`; paths in the reference column are relative to that repository.

[Fact] The starting Ferrin revision is `49f0e1b`. Its 903 passing tests establish the existing regression baseline, not proof of reference parity (source: the [capability completion verification](../03-engineering/04-testing.md)).

[Decision] Status describes review work, not released capability. `Not reviewed` and `In progress` must not be represented as verified. Record concrete regression evidence below as work completes; retain unresolved differences explicitly.

| Module | Existing surface | Reference | Review status |
| --- | --- | --- | --- |
| Provider specification | Provider traits, model families, content, usage and errors | `packages/provider/src` | Partial: embedding precision; remaining type-contract audit open |
| Schemas and JSON | Schema validation, dialect transforms, JSON parsing and repair | `packages/provider-utils/src` | Parsing/schema regressions added; async schema contract open |
| Application messages | Conversion, file sources and pruning | `packages/ai/src/prompt; packages/ai/src/generate-text/prune-messages.ts` | Conversion and pruning regressions added |
| Provider utilities | Transport, SSE, settings, retry classification and URL handling | `packages/provider-utils/src` | Streaming upload/retry regressions added; security differences retained |
| Tools and approval | Named contexts, callers, dynamic tools, approvals and replay | `packages/provider-utils/src; packages/ai/src/generate-text` | Runtime contract regressions added |
| Agent and hooks | Call preparation, options validation, hooks and timeout precedence | `packages/ai/src/agent` | Preparation, timeout and callback regressions added |
| Generation and step preparation | State, stop conditions, model/tool selection and step overrides | `packages/ai/src/generate-text` | State, sandbox and result regressions added |
| Streaming and structured output | Consumption, transforms, partial objects and array elements | `packages/ai/src/generate-text; packages/ai/src/text-stream` | View/output regressions added; ownership differences recorded |
| Middleware and registry | Wrappers, built-ins, provider lookup and defaults | `packages/ai/src/middleware; packages/ai/src/registry` | Regression fixes added; remaining differences below |
| Embedding and reranking | Batching, concurrency, ordering and result mapping | `packages/ai/src/embed; packages/ai/src/rerank` | Metadata/lifecycle regressions added; single-embedding differences open |
| Image, speech and transcription | Request preparation, batching, generated data and streaming | `packages/ai/src/generate-image; generate-speech; transcribe; translate` | Request/result/audio regressions added |
| Video, files, skills and batches | Polling, resources, request/result mapping and cancellation | `packages/ai/src/generate-video; upload-file; upload-skill; batch` | Request/resource regressions added; batch reference contract open |
| Realtime | Session lifecycle, events and tool execution | `packages/ai/src/realtime` | Session fixes added; event/state interface differences remain |
| MCP | Transports, protocol lifecycle, tool bridging and OAuth | `packages/mcp/src` | Transport/tool/app/OAuth regressions added; discovery/authentication differences remain |
| Telemetry and OpenTelemetry | Callback payloads, redaction, spans and metrics | `packages/ai/src/telemetry; packages/otel/src` | Awaited callbacks/modality spans added; hierarchy/payload differences remain |
| Policy | Decision normalization, defaults and shadow behavior | `packages/policy-opa/src` | Normalization, fallback and observer regressions added |
| OpenAI, Anthropic and compatible adapters | Existing model/resource families and provider tools | `packages/openai/src; anthropic/src; openai-compatible/src` | Tool-schema, request, upload and usage regressions added |
| Google, Azure and Voyage adapters | Existing model/resource families and provider tools | `packages/google/src; azure/src; voyage/src` | Tools, resources, options, authorization and ranking regressions added |
| Facade, macros and testing | Exports, feature composition, tool derivation and test helpers | `packages/ai/src/index.ts; packages/provider-utils/src; packages/test-server/src` | Facade/macro regressions and 68 feature builds pass; streaming recorder updated |

## Regression evidence (2026-09-17)

[Fact] The integration run of `just test` passed 1115 tests and skipped 10 credentialed live tests after callback, middleware, modality, MCP/OAuth and Google realtime changes. Workspace formatting, Clippy, doctests, rustdoc, docs-lint, typos, module-size, dependency usage and cargo-deny checks passed. All eighteen API snapshots were regenerated and `just api-check` passed. This does not establish complete behavioral parity or official-provider verification. PV-031 remains open.

[Fact] `just features` passed all 68 independent crate/feature/example builds. This caught and corrected the core's missing `futures-util/std` declaration and Google's unconditional event mapper using an optional schema dependency. No toolchain or external dependency versions changed.

[Fact] Agent/core evidence is in `crates/ferrin-core/tests/suite/{agent_options,agent_overrides,prepare_step_parity,tool_contract_parity,hooks,result_aggregation,prompt_conversion,prompt_instructions,stream_views,stream_transform_parity,output_parity,telemetry_async}.rs`. These cover optional options validation, prepared overrides, concurrent hooks, named contexts, dynamic calls, sandbox scope, independently consumed views and partial-output boundaries.

[Fact] Foundation evidence includes `ferrin-schema/tests/suite/{provider_schema,reference_partial,json}.rs`, `ferrin-message/tests/suite/prune.rs`, `ferrin-provider-util/tests/suite/upload_stream.rs` and `ferrin-testing/tests/suite/transport.rs`. The schema corpus contains 60 distinct reference repair inputs; provider factories are checked against 13 OpenAI, 20 Anthropic and seven Google tool schemas.

[Fact] Provider evidence includes each adapter's `tests/suite/reference_*` cases and fixture snapshots, plus Azure routing/security and Voyage reranking tests. Voyage's direct adapter now forwards empty documents and zero `top_n`, preserves parsed ranking order/duplicates/count/indices and leaves response model identity to the core. Its 14 tests passed after this correction. Fixture responses recorded through a proxy remain distinguished from official-provider responses.

## Confirmed work remaining

[Fact] Schema constructors and validators are synchronous (`ferrin-schema/src/schema.rs`); the reference accepts asynchronous schema production and validation (`provider-utils/src/{schema,validate-types}.ts`). The current parser fixes do not close this API difference.

[Fact] `CallOptions.tools: Vec<_>` does not represent omitted tools separately from an explicit empty list. Middleware defaults therefore cannot distinguish these inputs. Wrapper constructors also lack explicit identity options, and registry provider lists use sorted map order instead of registration order. Sources: `ferrin-spec/src/language_model/call_options.rs`, `ferrin-core/src/{middleware,registry}`; reference `ai/src/{middleware,registry}`.

[Fact] Single-value embedding reuses batch limits and requires an exact vector count, whereas reference `embed.ts` calls the provider directly and selects its first vector. This remains separate from the completed logical lifecycle work (`ferrin-core/src/embed.rs`).

[Fact] Core batch operations accept a bare `BatchId`; reference operations validate a versioned provider-bound batch reference. Realtime exposes a server-side stream and local tools rather than the reference session reducer/state/callback API. Sources: `ferrin-core/src/{batch,realtime}`, reference `ai/src/batch/batch.ts` and `ai/src/realtime`.

[Fact] MCP's 111 local tests pass after state/issuer validation, authorization-server credential binding and protocol-default corrections. Remaining OAuth differences include validated discovery redirects, protocol-header network retry, complete optional metadata validation and custom authentication on direct exchange/refresh helpers. File string shorthand also remains text rather than reference base64; explicit byte/text inputs are available. See [MCP](../01-architecture/15-mcp.md) and `ferrin-core/src/files.rs`.

[Fact] OpenTelemetry still lacks reference operation/step span hierarchy, supplemental attribute groups, enrichment callbacks and GenAI message formatting. Telemetry retains per-call registration, opt-in content recording and whole-context switches; reference global registration, recording defaults, per-property context filters and complete event payloads remain to be aligned. See [Observability](../01-architecture/13-observability.md).

## Representation and security boundaries

[Decision] Record Rust polling/ownership, JSON-only runtime/tool contexts, `usize` indices and Unicode-scalar/finite JSON-number representation explicitly. These are not evidence that arbitrary stricter adapter validation is equivalent. Shared completion and event views provide the supported Rust consumption contract without detached tasks.

[Decision] Retain HTTPS/private-network/DNS-pinning protections, bounded JSON/HTTP data, secret redaction and signed approvals under ADR 0026. Reasoning-stream cleanup still closes incomplete lifecycle events more strictly than the reference. The remaining source differences above prevent a claim that every existing module is strictly aligned.

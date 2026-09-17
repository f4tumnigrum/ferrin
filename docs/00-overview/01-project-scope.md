# Project scope

**English** | [Chinese](../zh-CN/00-overview/01-project-scope.md)

## 1. Positioning

Ferrin is an AI SDK delivered as Rust libraries. Its callers are Rust applications that depend directly on Ferrin crates: servers, CLIs, desktop application backends, and embedded agent processes. Ferrin provides:

- A unified interface independent of specific model providers: text generation, streaming, tool loops, structured output, embeddings, images, speech, transcription, reranking, video, file and skill uploads, batches, and realtime sessions.
- A stable provider specification that allows adapters to be implemented and released independently.
- A tool system covering local, provider-executed, dynamic, and MCP tools, with approval, repair, timeouts, and telemetry.
- Infrastructure for long-term maintenance: a multi-crate workspace, shared lints, fixture tests, an ADR process, and semantic versioning.

Ferrin is an independently implemented Rust library. Its design draws on the Vercel AI SDK's public source and documentation, and some code was ported from that project (see the root `NOTICE`, the README's Acknowledgments, and [ADR 0017](../04-decisions/2026-09-14-0017-apache-2-license-and-attribution.md) for licensing). Its interfaces follow Rust ownership, types, and async semantics without legacy API names, compatibility layers, migration paths for `experimental_` prefixes, or concurrent specification versions.

## 2. Functional scope

[Decision] Ferrin targets the following capability areas. Together, the surfaces exposed by major provider APIs (OpenAI, Anthropic, Google) and MCP form a complete, coherent SDK. Each area's behavior is defined by facts and decisions in its architecture chapter.

| Capability area | Ferrin crate |
| --- | --- |
| Provider specification (12 model/resource interface families) | `ferrin-spec` |
| Application message model | `ferrin-message` |
| Tool definitions and execution | `ferrin-tool`, `ferrin-core` |
| Schemas and JSON processing | `ferrin-schema` |
| HTTP, SSE, retry classification, secure URLs | `ferrin-provider-util` |
| Text generation loop, streaming, structured output, agents, middleware, registry, telemetry | `ferrin-core` |
| Embeddings, images, speech, transcription, reranking, video, files, skills, batches, realtime | `ferrin-core` |
| MCP client | `ferrin-mcp` |
| OpenTelemetry export | `ferrin-otel` |
| Policy-based tool approval (OPA REST Data API, embedded Rego) | `ferrin-policy` |
| Testing utilities | `ferrin-testing` |
| Provider adapters | `ferrin-openai`, `ferrin-anthropic`, `ferrin-openai-compatible`, `ferrin-google`, `ferrin-azure`, `ferrin-voyage` |

## 3. Non-goals

- Browser/frontend UI state management, such as React hooks. Ferrin provides serializable stream events for Rust services to forward to any frontend.
- Compatibility layers for existing frontend message protocols. Ferrin defines its own event serialization (see [Generation loop and streaming](../01-architecture/07-generation-loop-and-streaming.md)).
- Hosted gateways and implicit default providers (process-global default model resolution). Ferrin makes no network requests without explicit configuration.
- Workflow serialization and resumption (persisting an active generation loop to external storage and restoring it).
- Concrete sandbox environments (Docker, remote sandboxes). Ferrin defines only the `Sandbox` trait.
- Hosted agent runtime (harness) abstractions, which depend on specific hosting infrastructure and are outside Ferrin's scope.

## 4. Design principles

[Decision] These principles form the baseline for a Rust library maintained over the long term. Multi-crate workspaces require clear dependency direction (1, 2); published public APIs are subject to semver (3, 4); libraries should not make implicit choices for applications (5); documentation needs traceable sources to guide implementation (6).

1. Use the adapter pattern as the foundation. Application code depends on traits from `ferrin-spec`; adapters encapsulate provider differences and pass them through `provider_options`/`provider_metadata`.
2. Separate components. Messages, tools, schemas, HTTP utilities, and core loops belong to distinct crates that can be depended on and replaced independently.
3. Keep the public API conservative. Each crate exports explicitly; everything else is private. Register new public types in the API reference.
4. Apply the rule of three. Extract a public utility only after a pattern recurs at least three times.
5. Prefer explicit choices. Do not put configuration in global mutable state or encode modes with boolean or bare `Option` parameters; use enums and named methods.
6. Separate facts, decisions, and pending verification. Every statement must trace to source code or explicit technical rationale.

## 5. Version baseline

- Rust toolchain: 1.98.1 (the latest official stable release when the design was recorded; see [Toolchain and dependencies](../03-engineering/01-toolchain-and-dependencies.md) for verification records).
- Version policy: during `0.y.z`, minor releases may introduce breaking changes documented in the changelog; from `1.0`, follow semantic versioning (see [Versioning and release](../03-engineering/06-versioning-and-release.md)).

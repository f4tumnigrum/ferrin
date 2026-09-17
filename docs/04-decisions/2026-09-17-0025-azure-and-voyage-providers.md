# 0025: Azure OpenAI and Voyage providers

**English** | [Chinese](../zh-CN/04-decisions/2026-09-17-0025-azure-and-voyage-providers.md)

- Status: proposed
- Date: 2026-09-17
- Related: [Crate boundaries](../01-architecture/02-crates.md), [Provider specification](../01-architecture/04-provider-spec.md)

## Context

[Fact] The current provider set has no reranking implementation and no Azure-specific authentication or routing. The local AI SDK baseline (`6c6c221`, `packages/voyage/src/reranking` and `packages/azure/src`) supplies examples of these wire contracts.

## Decision

[Decision] Add `ferrin-voyage` for the Voyage rerank endpoint and `ferrin-azure` for Azure OpenAI Responses, Chat, Completions, embeddings, images, speech and non-streaming transcription. Expose additive facade features `voyage` and `azure`; keep the workspace release version unchanged until a separate release decision.

[Decision] Voyage depends on the existing specification and HTTP utilities, supports text and JSON documents (serialized with a compatibility warning), and rejects invalid response indices. Credentials are resolved lazily from `VOYAGE_API_KEY`.

[Decision] Azure reuses `ferrin-openai` model implementations. This is an explicit exception to the L3 dependency rule: reusing the protocol implementation avoids divergent parsing and security fixes. Azure supplies a private authenticated transport and per-deployment configuration, supporting v1 URLs, legacy deployment URLs, API keys and a per-request asynchronous Entra token provider. The OpenAI configuration gains an explicit external-authentication constructor so this integration never reads `OPENAI_API_KEY` or inserts a placeholder secret.

[Decision] Azure credentials are attached only to the configured origin and API path prefix. Returned download URLs retain the existing secure URL validation; Azure authentication must not be sent to unrelated hosts or same-origin paths. Token-provider failures are redacted. No external dependencies are added.

## Alternatives

[Decision] Bedrock and Vertex remain future adapters; Azure was selected by the project owner. Voyage embeddings are outside this change because the requested gap is reranking. A full fork of OpenAI model implementations would duplicate request/stream logic and is rejected.

## Consequences

[Decision] [ADR 0026](2026-09-17-0026-reference-sdk-parity.md) revises default credential precedence to the reference SDK: an Azure Authorization header skips Entra acquisition, and configured/call headers override generated credentials within the endpoint boundary. Voyage resolves the API key before applying header overrides. Origin/path isolation and redaction remain mandatory; this revision does not change this ADR's proposed status.

[Decision] Provider fixtures verify request/response translation only. Credentialed Azure and Voyage service verification remains part of PV-031; local tests must not be described as live provider verification. New crates require matching license/notice files, documentation, API snapshots and publication ordering. The ADR remains proposed pending maintainer review.

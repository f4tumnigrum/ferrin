# Voyage

**English** | [Chinese](../zh-CN/providers/voyage.md)

[Decision] `ferrin-voyage` implements Voyage's non-streaming rerank endpoint. `create_voyage(VoyageSettings)` returns a `VoyageProvider`; `reranking(model_id)` creates a concrete model and `Provider::reranking_model` supplies the dynamic model used by `rerank`. Other model kinds are unsupported. See [ADR 0025](../04-decisions/2026-09-17-0025-azure-and-voyage-providers.md).

## Configuration

[Decision] The default endpoint is `https://api.voyageai.com/v1`. Settings accept a base URL, API key, custom provider name, extra headers and HTTP transport. An API key is resolved lazily from explicit settings or `VOYAGE_API_KEY` before header overrides are applied. Per-call headers override configured headers; requests append the crate version to User-Agent. Base URLs must be HTTP(S) URLs without credentials, query strings or fragments; HTTP allows explicitly configured local test endpoints.

[Decision] Migration under [ADR 0026](../04-decisions/2026-09-17-0026-reference-sdk-parity.md): an Authorization header alone no longer bypasses API-key resolution. Supply an explicit key or `VOYAGE_API_KEY` even when replacing the outgoing Authorization header, matching the reference provider's evaluation order.

## Reranking

[Fact] The wire request uses `model`, `query`, `documents`, `top_k`, `return_documents` and `truncation`. Responses contain `data` entries with `index` and `relevance_score`. The contract is based on the local AI SDK baseline `6c6c221`, `packages/voyage/src/reranking/voyage-reranking-model.ts` and its options schema.

[Decision] Provider options under `voyage` accept camelCase `returnDocuments` and `truncation`, both optional booleans. Options under a custom provider name override canonical options. Unknown keys are ignored, matching the reference object schema; invalid known options fail before HTTP. The adapter does not constrain model IDs to a fixed model list.

[Decision] Text documents are sent unchanged. JSON objects are serialized as strings and emit one compatibility warning. Direct model calls forward empty document lists and zero `top_n`; the core's empty-list shortcut remains available. The adapter preserves parsed ranking entries in response order, including duplicates and results beyond the requested limit. The core validates indices before resolving original documents. Rust's index type rejects negative or fractional indices during response parsing. The raw body retains returned documents and usage; these do not become new specification fields. This replaces the additional adapter-level restrictions under ADR 0026.

[Decision] Adapter response metadata retains response headers and the raw body. The core supplies the requested model ID and timestamp; an opaque response `model` field remains only in the raw body. HTTP errors decode Voyage's `detail` message and use the shared status-based retry classification. Cancellation and transport behavior follow the shared HTTP implementation.

## Verification limits

[Pending verification] (PV-031) Local response fixtures and request snapshots verify translation and validation, including the core `rerank` path. They are handwritten contract examples, not captured Voyage service responses. Credentialed service verification and recording remain outstanding; see [Pending verification](../05-appendix/02-pending-verification.md).

## Implementation record (2026-09-17)

[Fact] The implementation is in `crates/providers/ferrin-voyage/src/`; `tests/suite/` contains ten passing local tests using wiremock and the injected recording transport. They cover options, authentication headers, redaction, malformed rankings, errors, cancellation, request snapshots and the core JSON-document rerank path. Package Clippy with warnings denied, two rustdoc examples and documentation generation with warnings denied passed on 2026-09-17. These runs verify the adapter locally, not the external service.

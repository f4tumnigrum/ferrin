# 0003: JSON values and serialization

**English** | [Chinese](../zh-CN/04-decisions/2026-09-13-0003-json-value-and-serialization.md)

- Status: accepted
- Date: 2026-09-13
- Related: [Core data model](../01-architecture/03-core-data-model.md)

## Context

[Fact] Arbitrary provider JSON options/metadata need grouped passthrough. Tagged objects are conventional cross-language union encoding for frontend events.

[Fact] OpenAI/Anthropic cache request prefixes including tools; changing tool/key order can miss caches, requiring stable serialization.

## Decision

1. Use serde_json Value/`Map`, not custom JSON types.
2. Enable `preserve_order` in spec.
3. Serialize tagged public enums with type or `role`, kebab-case tags, snake_case fields, base64 bytes, and RFC 3339 times.
4. Mark public enums and extensible structs non_exhaustive.

## Rationale

- `serde_json` is standard; custom types add conversion.
- Stable ordering makes tool definitions and fixtures deterministic.
- Internal tags support common frontend JSON consumers.

## Alternatives

- Custom `JsonValue` isolates the API from the ecosystem.
- External serde tags differ from common SSE/JSON events.

## Consequences

- Document downstream feature unification of `preserve_order`.
- Downstream non_exhaustive matches need wildcards.

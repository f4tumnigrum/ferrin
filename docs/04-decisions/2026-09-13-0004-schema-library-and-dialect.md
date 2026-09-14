# 0004: Schema library and dialect

**English** | [Chinese](../zh-CN/04-decisions/2026-09-13-0004-schema-library-and-dialect.md)

- Status: accepted
- Date: 2026-09-13
- Related: [Structured output](../01-architecture/08-structured-output.md), [Tool system](../01-architecture/06-tool-system.md)

## Context

[Fact] Providers describe tools/output through JSON Schema subsets. OpenAI strict mode requires no additional properties and all fields `required`; Google uses an OpenAPI 3.0 subset. Adapters need transforms.

## Decision

1. Derive with `schemars` 1.2.2, draft-07 by default, and validate typed values through `serde`.
2. Validate dynamic schemas with optional `json-schema-validation` and `jsonschema` 0.56.0.
3. `Schema<T>` combines lazy JSON Schema and validator, from derives, raw schemas, or custom validation.

## Rationale

- Schemars integrates widely with `serde`, eliminating duplicate type/schema declarations.
- Draft-07 `definitions`/`type` arrays are broadly supported and need few keyword transforms.
- Optional dynamic validation avoids its dependency cost for applications not needing it.

## Alternatives

- Raw-only schemas risk drift from handwritten Rust types.
- Draft 2020-12 would require revalidating all adapter transforms.

## Consequences

- [Fact] PV-004 verified optional primitive/reference and `enum` shapes plus OpenAI `required`/no-extra/nullability transformation in `verification/pv004-schema`; see [Tool system](../01-architecture/06-tool-system.md), section 10.
- Include public `schemars::JsonSchema` bounds in the third-party allowlist.

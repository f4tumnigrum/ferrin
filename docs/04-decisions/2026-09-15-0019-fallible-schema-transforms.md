# 0019: Fallible provider schema transforms

**English** | [Chinese](../zh-CN/04-decisions/2026-09-15-0019-fallible-schema-transforms.md)

- Status: accepted
- Date: 2026-09-15
- Related: [ADR 0004](2026-09-13-0004-schema-library-and-dialect.md), [Tool system](../01-architecture/06-tool-system.md)

## Context

[Fact] OpenAI strict objects require `additionalProperties: false` ([structured outputs guide](https://platform.openai.com/docs/guides/structured-outputs)). An arbitrary-key dictionary represented by a schema-valued `additionalProperties` cannot keep its values under that restriction.

[Fact] The original infallible `SchemaTransform` API preserved dictionary schemas unchanged, so its OpenAI strict result could still violate that requirement (review F04; regression cases in `crates/ferrin-schema/tests/suite/transform.rs`).

## Decision

[Decision] `SchemaTransform::apply`, `SchemaTransform::applied`, and `to_openai_strict` return `Result<_, SchemaError>`. OpenAI strict rejects schema-valued or explicitly true `additionalProperties` and `patternProperties` anywhere in a schema, including schema-valued draft-07 `dependencies`, before mutation, using `SchemaError::UnsupportedTransform`. Applications can disable strict mode or supply a supported representation themselves.

[Decision] `Schema::transformed` also returns `Result` and evaluates the transformation immediately, preserving the existing validator. Default derived schemas keep their infallible additional-properties transform, which preserves dictionary schemas.

## Rationale and alternatives

[Decision] Do not silently remove dictionary values or replace an object with key/value entries: either would change the application's input/output representation. Keeping an infallible API would require a panic or a knowingly unsupported output; explicit errors let providers fail before a request is sent.

## Consequences

[Decision] This is a breaking Rust API change: callers propagate or handle the new `Result`. Existing infallible free helpers for additional-properties closure and property-name removal remain available. This refines ADR 0004 without changing the default generation dialect or the typed validator.

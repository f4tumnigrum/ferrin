# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Changed

- Synchronize bundled attribution with the Azure and Voyage adapter additions.

## [0.1.2] - 2026-09-16

### Changed

- Coordinate workspace version 0.1.2 and synchronize the packaged attribution notice; no public API changes.

## [0.1.1] - 2026-09-15

### Changed

- **Breaking:** `SchemaTransform::apply`/`applied`, `to_openai_strict`, and
  `Schema::transformed` return `Result`, rejecting unsupported strict
  dictionaries, including draft-07 schema dependencies, without changing their representation ([ADR 0019](../../docs/04-decisions/2026-09-15-0019-fallible-schema-transforms.md)).

### Fixed

- Repair JSON prefixes containing escaped object keys and Unicode surrogate
  pairs without committing incomplete scalars; preserve positive-exponent
  numbers in complete documents and truncated prefixes.

- Preserve nullability of optional enum, const, and composed constraints in
  OpenAI strict schemas without widening their non-null values or breaking
  references to declared schema resources when nullable wrappers move nodes.

- Honor the declared JSON Schema dialect during dynamic validation, using
  draft-07 only when `$schema` is absent.

### Added

- `partial_json` benchmark (`repair`, `parse_partial` on document prefixes,
  `serde_json` baseline) and `schema` benchmark (derivation, `openai_strict`,
  typed and raw JSON Schema validation).

## [0.1.0] - 2026-09-14

### Added

- `Schema<T>`: lazily generated JSON Schema plus typed validator; constructors
  `derived`/`derived_with` (schemars + serde, `additionalProperties: false`),
  `from_json_schema`, `typed_from_json_schema`, `lazy`,
  `with_json_schema_and_validator`, `empty_object`, `any`; adapters
  `with_validator`, `transformed`, `erased`.
- `SchemaDialect` (draft-07 default, 2020-12) wrapping `schemars` settings.
- `SchemaTransform` (`AdditionalPropertiesFalse`, `RemovePropertyNames`,
  `OpenAiStrict`) and the underlying free functions.
- `partial_json::{repair, parse_partial}` porting the streaming JSON repair
  state machine, property-tested over prefixes of arbitrary JSON.
- `json::{parse, parse_with, parse_with_schema, is_parsable, depth}` with
  `ParseLimits` (depth 128, 64 MiB).
- `validation::{ValidationIssue, ValidationIssues}` and, behind the
  `json-schema-validation` feature, a draft-07 `Validator` built on
  `jsonschema`.
- `SchemaError` with conversion into `ferrin_spec::ProviderError`.

### Changed

- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0); `LICENSE` and
  `NOTICE` are included in the package (ADR 0017).
- Crate and module documentation attribute the code derived from the Vercel
  AI SDK.

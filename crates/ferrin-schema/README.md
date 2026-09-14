# ferrin-schema

JSON Schema support for Ferrin tool inputs and structured output.

- `Schema<T>`: a JSON Schema plus a typed validator, derived from a Rust type
  (`schemars` + `serde`, `additionalProperties: false` by default), built from
  a raw JSON Schema, or assembled from a custom validator; adapters for
  transforming, erasing and re-validating schemas.
- `SchemaDialect`: draft-07 (default) or 2020-12 generation settings.
- `SchemaTransform`: provider-oriented rewrites such as
  `AdditionalPropertiesFalse` and `OpenAiStrict`.
- `partial_json`: repair and parse truncated JSON emitted by streaming
  models; `json`: parsing with size and depth limits.
- Re-exports `schemars` so downstream derives can use
  `#[schemars(crate = "ferrin_schema::schemars")]`.

## Features

| Feature | Default | Effect |
|---|---|---|
| `json-schema-validation` | on | validates raw JSON Schemas with `jsonschema` when a `Schema` is built from JSON |

Part of the [Ferrin](https://github.com/f4tumnigrum/ferrin) workspace. Design:
`docs/01-architecture/08-structured-output.md`, ADR 0004.

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). Portions of this crate are derived from the Vercel AI SDK (Apache-2.0); the crate and module documentation carry the attribution.

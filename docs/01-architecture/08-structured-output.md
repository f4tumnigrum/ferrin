# Structured output

**English** | [Chinese](../zh-CN/01-architecture/08-structured-output.md)

Structured output lives in `ferrin-core::output`; schemas and partial JSON repair live in `ferrin-schema`.

## 1. Output strategies

[Decision] `Output` defines five strategies, each supplying response format, complete parsing, and partial parsing:

| Strategy | Response format | Complete parsing | Partial parsing |
| --- | --- | --- | --- |
| Text | `text` | Original `text` | Original `text` |
| Object (schema, optional name and description) | `json` with schema | Parse and validate; failure is `NoObjectGenerated` | Repair partial JSON and return deeply partial values |
| Array (element schema) | `json` with `{elements: [element]}` wrapper schema | Extract `elements` after parsing | Emit completed `elements` individually |
| Choice (candidate list) | `json` with `{result: {enum: options}}` wrapper | Extract `result` | Return a candidate when the prefix is unambiguous |
| Arbitrary JSON (optional schema) | `json` | Any JSON value | Repaired partial value |

[Decision] Use only `generate_text(...).output(Output::object::<T>())`, without a separate `generate_object`. One path avoids duplicate APIs and generation loops with identical capabilities.

```rust
pub struct Output<T> { strategy: OutputStrategy, _marker: PhantomData<T> }

impl Output<String> { pub fn text() -> Self; }
impl<T: DeserializeOwned + JsonSchema> Output<T> { pub fn object() -> Self; pub fn object_with(schema: Schema<T>) -> Self; }
impl<T: DeserializeOwned + JsonSchema> Output<Vec<T>> { pub fn array() -> Self; }
impl Output<String> { pub fn choice(options: impl IntoIterator<Item = impl Into<String>>) -> Self; }
impl Output<JsonValue> { pub fn json() -> Self; pub fn json_with_schema(schema: JsonValue) -> Self; }
```

`.output(Output<T>)` changes the result of a `GenerateText<O>` builder to `GenerateTextResult<T>`.

## 2. Parsing conditions

[Decision] Parse only if the final step's finish reason is `stop`, or is not `tool-calls` and text is nonempty. Otherwise return `NoOutputGenerated`. Parsing failures are `NoObjectGenerated`, carrying text, response, usage, finish reason, and cause. Steps ending in tool calls lack a final answer and should not produce spurious parse failures.

[Decision] Unsatisfied conditions return `Error::NoOutputGenerated { steps }` instead of empty `output`. Since `output` is `T`, not `Option<T>`, callers must handle the missing structured result explicitly.

## 3. Partial JSON repair

[Decision] A state machine scans input with a stack of objects, arrays, strings, literals, and numbers. At truncation, it closes quotes/brackets and removes incomplete literals (such as `tru`) and trailing commas. Try parsing directly, then repair and parse, reporting success, repaired success, or failure. Stack-based repair handles nested streaming structures more reliably than regex replacement.

[Decision] Port this state machine as `ferrin_schema::partial_json::repair(&str) -> Cow<str>` and `parse_partial(&str) -> PartialParse { value: Option<JsonValue>, state }`. Use `proptest` to verify that repaired prefixes of valid JSON are parseable prefix approximations of the original.

[Fact] Implementation on 2026-09-13: `PartialParseState` has `SuccessfulParse`, `RepairedParse`, and `FailedParse`; empty input fails. `[-` repairs to `[]`, not invalid `[-]`. Property tests check every character-boundary prefix of random compact and pretty-printed JSON.

## 4. Partial output streams

[Decision] Reparse accumulated text after each text delta and emit only changed values; array strategies also emit newly completed elements. Deduplication avoids unnecessary UI rendering.

```rust
pub struct PartialOutput<T> {
    pub value: JsonValue,           // repaired partial JSON
    pub typed: Option<T>,           // Some when the partial value already deserializes into T
}
```

[Decision] Partial output is primarily JSON, with typed values as a supplement. Rust cannot recursively make all fields optional at the type level without separate types. Applications can preview partial JSON and consume typed complete values. Arrays offer `element_stream() -> impl Stream<Item = T>`.

## 5. Schema

### 5.1 Abstraction

[Decision] `Schema<T>` combines a lazily computed JSON Schema and validator, constructed from Rust types via `schemars`, handwritten schemas, or custom validation. Binding the schema sent to providers to local validation prevents inconsistencies.

```rust
pub struct Schema<T> {
    json_schema: LazySchema,                       // OnceLock<JsonValue> or precomputed
    validate: Arc<dyn Fn(JsonValue) -> Result<T, TypeValidationError> + Send + Sync>,
}

impl<T: DeserializeOwned + JsonSchema> Schema<T> {
    pub fn derived() -> Self;                       // schemars + serde
}
impl Schema<JsonValue> {
    pub fn from_json_schema(schema: JsonValue) -> Self;   // dynamic; validates via `jsonschema` when the feature is on
}
impl<T> Schema<T> {
    pub fn with_validator(self, f: impl Fn(JsonValue) -> Result<T, TypeValidationError> + Send + Sync + 'static) -> Self;
    pub fn json_schema(&self) -> &JsonValue;
    pub fn validate(&self, value: JsonValue) -> Result<T, TypeValidationError>;
}
```

[Fact] The 2026-09-13 implementation adds `Schema::<T>::typed_from_json_schema(JsonValue)` (deserialize as `T`, first validating JSON Schema when enabled), `Schema::lazy(FnOnce() -> JsonValue, validator)`, `with_json_schema_and_validator`, `Schema::<JsonValue>::empty_object()`/`any()`, `transformed(SchemaTransform)` (lazy schema rewrite, unchanged validator), and `erased() -> Schema<JsonValue>` (run the original validator, return original JSON). `json_schema` uses `LazyLock<JsonValue, Box<dyn FnOnce>>` shared by `Arc`; clones share cache and validator. Without `json-schema-validation`, `from_json_schema` accepts all values.
### 5.2 Schema dialect

[Fact] Providers accept JSON Schema subsets, requiring adapter transforms: Anthropic removes unsupported keywords; OpenAI strict mode requires `additionalProperties: false` and all properties in `required`.

[Decision] Default to `schemars` draft-07 because its `definitions` and `type` arrays are widely supported by provider parsers ([ADR 0004](../04-decisions/2026-09-13-0004-schema-library-and-dialect.md)). Applications may override `SchemaSettings` to 2020-12.

### 5.3 JSON parsing security

[Fact] JSON keys such as `__proto__`/`constructor.prototype` can cause prototype pollution in JavaScript. `serde_json` uses ordinary Rust maps, so Ferrin needs no prototype-pollution parsing step.

[Decision] `ferrin_schema::json::parse` instead enforces resource limits: default nesting depth 128 and maximum 64 MiB (HTTP separately limits provider bodies). Exceeding limits returns `JsonParseError`. An explicit depth matches `serde_json`'s default and makes configuration documented.

## 6. Example

```rust
#[derive(Debug, Deserialize, JsonSchema)]
struct Recipe {
    name: String,
    ingredients: Vec<String>,
    steps: Vec<String>,
}

let result = ferrin::generate_text(&model)
    .prompt("Generate a lasagna recipe.")
    .output(Output::<Recipe>::object())
    .await?;

println!("{} ingredients", result.output.ingredients.len());
```

Streaming:

```rust
let stream = ferrin::stream_text(&model)
    .prompt("Generate a lasagna recipe.")
    .output(Output::<Recipe>::object())
    .await?;

let mut partials = stream.partial_output_stream();
while let Some(partial) = partials.next().await {
    if let Some(recipe) = partial.typed {
        println!("complete so far: {}", recipe.name);
    }
}
```

## 7. Verification items

- [Fact] (PV-004) `schemars` draft-07 generates `type: [T, "null"]` for optional primitives, `anyOf: [$ref, {type: null}]` for optional references, and `enum`/`oneOf` for enums. OpenAI strict and Anthropic `sanitize_json_schema` transforms handle these shapes; see [Tool system](06-tool-system.md), section 10.
- [Fact] (PV-008, `verification/pv008-partial-compare`, release build) Deep equality on `serde_json::Value` takes 16 µs for 9 KiB, 135 µs for 96 KiB, and 675 µs for 507 KiB objects. Serializing then hashing is slower (18/180/916 µs); parsing costs 6–7 times more than comparison (93 µs/936 µs/4.7 ms).
- [Decision] Keep deep `Value` equality rather than text hashing: comparison is roughly an order of magnitude cheaper than parsing, and hashing is slower. If performance becomes an issue, optimize incremental parsing.

[Decision] Dynamic JSON Schema validation selects the dialect declared by `$schema`; only schemas without a declaration default to draft-07. Dialect-specific constraints must be evaluated rather than silently treated as unknown keywords.

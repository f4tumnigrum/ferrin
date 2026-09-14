# ferrin-spec

Ferrin provider specification: the traits every provider implements and the
types they exchange with the core.

- Model interfaces: `LanguageModel` (`do_generate`/`do_stream`),
  `EmbeddingModel`, `ImageModel`, `SpeechModel`, `TranscriptionModel`,
  `RerankingModel`, `VideoModel`, `SpeechTranslationModel`, `RealtimeModel`
  and `RealtimeFactory`; services `Files`, `Skills` and `Batch`; the
  `Provider` trait that hands them out by model id.
- Object-safe `Dyn*` counterparts and the `ModelRef`/`ServiceRef` handles
  (`LanguageModelRef`, `EmbeddingModelRef`, ...) accepted by the core entry
  points.
- Wire-level data model: `Prompt` and content parts, `CallOptions`,
  `ToolDefinition`, `Content`, `StreamPart`, `GenerateResult`, `Usage`,
  `FinishReason`, `Warning`, `Headers`, `ProviderOptions`/`ProviderMetadata`,
  newtype identifiers and the error types (`ProviderError`, `ApiCallError`,
  `TypeValidationError`, ...).
- `SPEC_VERSION`: the specification version, equal to the crate version
  (ADR 0011).

Async trait methods are written as `fn name(..) -> impl Future<..> + Send`;
implementations may use `async fn`.

Part of the [Ferrin](../../README.md) workspace. Design:
`docs/01-architecture/03-core-data-model.md`,
`docs/01-architecture/04-provider-spec.md`,
`docs/02-api/02-api-reference.md`.

## License

MIT OR Apache-2.0.

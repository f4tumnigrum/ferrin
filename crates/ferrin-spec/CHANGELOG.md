# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- Shared types: newtype identifiers (`ProviderId`, `ModelId`, `ToolName`,
  `ToolCallId`, `ApprovalId`, `PartId`, `BatchId`), `ProviderOptions`,
  `ProviderMetadata`, `ProviderReference`, `Warning`, `Headers` (merge,
  user-agent suffix, masked debug/serde output), `MediaType`, `FileData`
  (`type`-tagged `data`/`url`/`reference`/`text`), `AudioFormat`.
- Language model interface: `LanguageModel` trait, `CallOptions` (+ serializable
  `CallOptionsRecord`), specification `Prompt`, `ToolDefinition`, `Content`,
  `StreamPart`/`StreamError`, `GenerateResult`/`StreamResult`, `Usage`,
  `FinishReason`, `SupportedUrls`.
- Embedding, image, speech, transcription, reranking, video, speech translation
  and realtime model interfaces; `Files`, `Skills` and `Batch` services;
  `Provider` trait.
- `dynamic` module: object-safe `Dyn*` traits with blanket implementations,
  `BoxFuture`/`BoxStream`, `ModelRef` (resolved instance or unresolved id) and
  `ServiceRef` handles.
- `error` module: `ProviderError` (≤ 128 bytes, includes `Cancelled`) and concrete error structs
  mirroring the provider specification; `TypeValidationError` keeps its
  context boxed so `Result<T, TypeValidationError>` stays below the
  large-error threshold.

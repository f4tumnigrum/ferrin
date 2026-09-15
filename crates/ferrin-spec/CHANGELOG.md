# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.2] - 2026-09-16

### Changed

- Coordinate workspace version 0.1.2 and synchronize the packaged attribution notice; no public API changes.

## [0.1.1] - 2026-09-15

### Fixed

- SPEC: Redact Realtime WebSocket URLs and protocols in `Debug` output so
  connection credentials remain secret ([ADR 0009](../../docs/04-decisions/2026-09-13-0009-http-transport-and-secure-url.md)).

## [0.1.0] - 2026-09-14

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

### Changed

- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0); `LICENSE` and
  `NOTICE` are included in the package (ADR 0017).
- Crate and module documentation attribute the code derived from the Vercel
  AI SDK.

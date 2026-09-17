# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- General Interactions language models with structured/multimodal output,
  function/provider tools, linked history, incremental SSE, resumable background
  streams, and start/get/cancel resource operations (ADR 0023).
- Live streaming transcription and speech translation behind `realtime`, with
  secure DNS-pinned WebSockets, setup gating, cancellation, bounded messages,
  backpressure and explicit completion handling.


### Changed

- Synchronize bundled attribution with the Azure and Voyage adapter additions.

### Fixed

- Ignore normalized Realtime events without a Live API equivalent, preserving
  automatic tool-response continuation; retain JSON tool output values directly
  and replace invalid JSON with an empty object (ADR 0026).
- Ignore unknown provider-option keys as the reference object schemas do, while
  continuing to validate known option fields.
- Match reference input/output object parsing for all seven existing tool factories.
- Align Interactions tool fields and defaults, including provider-executed MCP,
  native built-in result replay, linked-history compaction and option validation.
- Normalize Interactions output formats, agent environments, adjacent user text
  and video processing options using the reference request contracts.
- Validate embedding, speech, transcription and image options; preserve optional
  speech payload fields and reject missing embedding response arrays.
- Encode file resource names, enforce polling options and cancellation, preserve
  batch zero counters, and use configured same-origin video credentials.
- Match reference image call-limit reporting and omission of zero video settings.

## [0.1.2] - 2026-09-16

### Changed

- Coordinate workspace version 0.1.2 and synchronize the packaged attribution notice; no public API changes.

## [0.1.1] - 2026-09-15

### Fixed

- Validate and pin server-provided upload URLs, restrict cross-origin headers, and bound finalization responses.

- Preserve executable-code calls and results when replaying provider-executed tools.

- Preserve unconstrained boolean schemas and reject unrepresentable false schemas.

- Preserve plain-text and non-object JSON tool outputs in Live API responses.

- Retain thought signatures when replaying generated files and reasoning files.

- Preserve explicit thinking budgets and levels when generic reasoning is also set.

- Report and enforce a single-image call limit so core can split multi-image requests.
- Reject truncated SSE streams that end without an explicit completion signal.

### Added

- `generate_content` benchmark: `do_generate` and `do_stream` against the
  fixture server replaying the `text`, `text` stream and `reasoning` stream
  fixtures.

## [0.1.0] - 2026-09-14

### Added

- `create_google` / `GoogleSettings` / `GoogleProvider` with lazily loaded
  `GOOGLE_GENERATIVE_AI_API_KEY` (`x-goog-api-key`), a provider name
  override and the `ferrin-google/<version>` user-agent suffix.
- `generateContent` language model (`GoogleLanguageModel`): prompt
  conversion (inline and file data, thought signatures, Gemini 3 function
  response parts, server tool replay), provider options, thinking level and
  budget mapping, JSON Schema to OpenAPI conversion with a
  `parametersJsonSchema` fallback, tool and tool-choice conversion, generate
  and stream mapping including grounding sources, code execution, inline
  files and streamed `partialArgs`.
- Provider tool factories (`GoogleTools`): Google Search, enterprise web
  search, URL context, code execution, file search, Vertex RAG store and
  Google Maps.
- Embedding (`GoogleEmbeddingModel`), image (`GoogleImageModel`), speech
  (`GoogleSpeechModel`), transcription (`GoogleTranscriptionModel`) and video
  operation (`GoogleVideoModel`) models.
- Files API (`GoogleFiles`: resumable upload with processing poll, metadata,
  delete), batch generation (`GoogleBatch`: start with inline or uploaded
  JSONL input, status, results stream, cancel, list) and Live API sessions
  (`GoogleRealtimeModel`, `GoogleRealtimeFactory`: ephemeral tokens, setup
  message, bidirectional event mapping).
- Fixture-driven test suite with stream contract checks and request,
  prompt, tool, schema and stream snapshots.

### Changed

- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0); `LICENSE` and
  `NOTICE` are included in the package (ADR 0017).
- Crate and module documentation attribute the code derived from the Vercel
  AI SDK.

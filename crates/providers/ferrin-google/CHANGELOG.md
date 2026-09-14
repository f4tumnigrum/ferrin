# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

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

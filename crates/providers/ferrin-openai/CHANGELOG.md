# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Added

- `create_openai` / `OpenAiSettings` / `OpenAiProvider` with lazily loaded
  `OPENAI_API_KEY`, `OPENAI_BASE_URL` fallback, organization and project
  headers, a provider name override and the `ferrin-openai/<version>`
  user-agent suffix.
- Responses API language model (`OpenAiResponsesLanguageModel`): prompt and
  tool conversion, provider options, reasoning, structured output, provider
  tools, generate and stream mapping, batch request bodies.
- Chat Completions (`OpenAiChatLanguageModel`) and legacy Completions
  (`OpenAiCompletionLanguageModel`) language models.
- Embedding, image (generations and edits), speech and transcription models.
- Files, skills and batch APIs; realtime client secrets, session
  configuration and event mapping (`OpenAiRealtimeModel`,
  `OpenAiRealtimeFactory`).
- Provider tool factories (`OpenAiTools`).
- `realtime` feature: WebSocket streaming transcription and
  `OpenAiSpeechTranslationModel`.
- Fixture-driven test suite with stream contract checks and request
  snapshots.

### Changed

- The stream driver (`StreamMachine`, `drive_stream`, `EarlyChunk`) moved to
  `ferrin_provider_util::stream_driver`; `stream_util` re-exports it and
  keeps the OpenAI error mapping and formatting helpers.
- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0); `LICENSE` and
  `NOTICE` are included in the package (ADR 0017).
- Crate and module documentation attribute the code derived from the Vercel
  AI SDK.

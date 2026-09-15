# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.1] - 2026-09-15

### Fixed

- Apply fallible strict schema transforms to Responses and Chat tool inputs and structured outputs.

- Preserve complete raw Chat usage in both generated and streamed results.

- Request base64 image edits and the single-image multipart field for DALL-E models.

- Reject truncated SSE streams that end without an explicit completion signal.

- Preserve opaque dictionary keys when converting provider tool options to wire format.

- Resolve Responses and Chat file references using the configured provider name.

- Encode local shell results with the matching Responses shell output wire type.

- Preserve individual custom tool names and application aliases during Responses replay.

- Send newly executed local tool results when a Responses conversation is configured.
- Verify that Realtime WebSocket configuration debug output redacts its token
  while retaining the authentication subprotocol for the connection.

### Added

- `responses` benchmark: `do_generate` and `do_stream` against the fixture
  server replaying the `text-basic`, `text-basic-stream` and
  `reasoning-stream` fixtures.

## [0.1.0] - 2026-09-14

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

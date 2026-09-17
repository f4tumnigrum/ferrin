# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Changed

- Synchronize bundled attribution with the Azure and Voyage adapter additions.

### Fixed

- Ignore unknown provider-option keys as the reference object schemas do, while
  continuing to validate known option fields.
- Match the reference schemas for all twenty existing provider tools, including
  union variants, defaults, object parsing and request argument validation.
- Stream multipart file uploads with cancellation and propagate input-stream
  failures without buffering the complete upload.

- Bind modern code-execution tools as provider callers and enable deferred results while preserving existing caller options.

## [0.1.2] - 2026-09-16

### Changed

- Coordinate workspace version 0.1.2 and synchronize the packaged attribution notice; no public API changes.

## [0.1.1] - 2026-09-15

### Fixed

- Validate and pin server-provided batch result URLs, restrict cross-origin headers, and limit downloaded bytes.

- Share the four-cache-breakpoint limit across prompt parts and tool definitions.

- Restore provider tool aliases in generated and streamed calls and map forced tool choices.

- Preserve definitions and reference scopes while sanitizing structured-output schemas.

- Accept documented camelCase block-binding options while preserving the legacy spelling.

### Added

- `messages` benchmark: `do_generate` and `do_stream` against the fixture
  server replaying the `text-basic`, `text-basic-stream` and
  `reasoning-stream` fixtures.

## [0.1.0] - 2026-09-14

### Added

- `create_anthropic` / `AnthropicSettings` / `AnthropicProvider` with lazily
  loaded `ANTHROPIC_API_KEY` or `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_BASE_URL`
  fallback (a bare origin gets `/v1`), a provider name override, merged
  `anthropic-beta` headers and the `ferrin-anthropic/<version>` user-agent
  suffix.
- Messages API language model (`AnthropicMessagesLanguageModel`): prompt
  conversion (images, PDFs, text documents, file references, container
  uploads, citations, cache control, mid-conversation system messages),
  provider options, extended thinking and effort mapping, native structured
  output with a `json` tool fallback, tool conversion, generate and stream
  mapping including server tool results, citations as sources, containers,
  context management and usage iterations.
- Provider tool factories (`AnthropicTools`): bash, computer, text editor,
  memory, web search, web fetch, code execution, tool search and advisor.
- Files (`AnthropicFiles::upload_file`), skills (`AnthropicSkills`) and the
  Message Batches API (`AnthropicBatch`: start, status, results stream,
  cancel, list).
- `AnthropicConfig::{supports_strict_tools, supports_native_structured_output}`
  for compatible endpoints.
- Fixture-driven test suite with stream contract checks and request,
  prompt, tool and stream snapshots.

### Changed

- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0); `LICENSE` and
  `NOTICE` are included in the package (ADR 0017).
- Crate and module documentation attribute the code derived from the Vercel
  AI SDK.

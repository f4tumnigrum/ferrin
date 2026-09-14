# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.0] - 2026-09-14

### Added

- `create_openai_compatible` and `OpenAiCompatibleSettings` (`name`,
  `base_url`, `api_key`, `api_key_env`, `headers`, `query_params`,
  `include_usage`, `supports_structured_outputs`, `supported_urls`,
  `max_embeddings_per_call`, `supports_parallel_calls`, `transport`,
  `id_generator`) and `OpenAiCompatibleProvider` with `Provider` support.
- Chat Completions language model (`<name>.chat`): non-streaming and
  streaming generation, function tools and tool choice, `json_schema` and
  `json_object` response formats, reasoning content, tool call deltas
  buffered until the function name arrives, thought signatures, usage
  details and prediction token metadata.
- Legacy Completions language model (`<name>.completion`), embedding model
  (`<name>.embedding`) and image model (`<name>.image`, JSON generation and
  multipart edits).
- Provider option key resolution (`openai-compatible` deprecated,
  `openaiCompatible`, `<name>`, camelCase `<name>`) with pass-through of
  unknown keys and deprecation warnings.
- Extension points for dedicated provider crates: `ErrorStructure`,
  `MetadataExtractor` / `StreamMetadataExtractor`, `transform_request_body`
  and `convert_usage`.
- Error frame mapping shared by the streaming models: HTTP status inferred
  from `code` / `type`, early error frames fail the call, later frames end
  the stream with a terminal error part.

### Changed

- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0); `LICENSE` and
  `NOTICE` are included in the package (ADR 0017).
- Crate and module documentation attribute the code derived from the Vercel
  AI SDK.

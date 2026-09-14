# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.0] - 2026-09-14

### Added

- `Message` (`role`-tagged system/user/assistant/tool), `UserContent` /
  `AssistantContent` (string or parts), constructors (`system`, `user`,
  `user_parts`, `assistant`, `assistant_parts`, `tool`), `Role`,
  `MessagesExt::{push_approval_response, pending_approval_requests}`.
- Parts: `UserPart` (text/image/file with `image_*`/`file_*` constructors),
  `AssistantPart`, `ToolPart`, `ImagePart`, `FilePart`, `ReasoningFilePart`,
  `ToolApprovalRequest`, `ToolApprovalResponse`; specification-identical parts
  re-exported from `ferrin-spec`.
- `FileSource` (`data`/`base64`/`url`/`reference`/`text`/`path`), lossless
  `From<FileData>`, I/O-free `TryFrom<FileSource> for FileData`,
  `decoded_bytes`, `data_url`.
- `data_url::parse` implementing RFC 2397 (first-comma split, case-insensitive
  `base64`, percent-decoding, optional padding, default media type).
- `prune::prune` with `PruneOptions` (reasoning, tool-call rules by scope and
  tool, empty message handling), ported from the reference algorithm.
- `InvalidDataContentError`, `FileSourceError`.

### Changed

- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0); `LICENSE` and
  `NOTICE` are included in the package (ADR 0017).
- Crate and module documentation attribute the code derived from the Vercel
  AI SDK.

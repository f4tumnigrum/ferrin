# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

### Changed

- Align the development version with workspace 0.1.1; no macro API changes.

## [0.1.0] - 2026-09-14

### Added

- `#[ferrin::tool]`: expands a function with one owned input parameter and an
  optional `ToolContext` into `fn name() -> ferrin::tool::Tool`
  (`Tool::function::<Input>()` with the doc comment as description); rejects
  reference parameters, explicit lifetimes, generics, `self`, missing return
  types and macro arguments with targeted compile errors.

### Changed

- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0); `LICENSE` and
  `NOTICE` are included in the package (ADR 0017).

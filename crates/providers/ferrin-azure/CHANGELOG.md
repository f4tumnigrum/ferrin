# Changelog

## [Unreleased]

### Added

- Azure OpenAI model factories with v1 and legacy deployment routing, lazy API
  keys, per-request Microsoft Entra tokens and credential origin scoping (ADR 0025).

### Changed

- Synchronize bundled attribution with the Azure and Voyage adapter additions.

### Fixed

- Honor explicit credential-header overrides using the reference precedence;
  skip Entra token acquisition when Authorization is already present, while
  preserving credential origin scoping.

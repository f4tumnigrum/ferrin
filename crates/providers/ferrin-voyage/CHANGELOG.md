# Changelog

All notable changes to this crate are documented here.

## [Unreleased]

### Added

- Voyage reranking provider with lazy credentials, configurable transport,
  provider options, JSON-document compatibility warnings and validated rankings.
- Local response fixtures, request snapshots and core rerank integration coverage;
  live Voyage service verification remains outstanding (PV-031).

### Changed

- Match reference direct-call behavior for empty documents, zero `top_n`,
  ranking order and duplicates. Index bounds are validated by the core before
  resolving original documents. Only the core supplies response model identity.
- Synchronize bundled attribution with the Azure and Voyage adapter additions.

### Fixed

- Ignore unknown provider-option keys as the reference object schemas do, while
  continuing to validate known option fields.
- Resolve the API key before applying Authorization header overrides, matching
  reference missing-key behavior and keeping credential values redacted.

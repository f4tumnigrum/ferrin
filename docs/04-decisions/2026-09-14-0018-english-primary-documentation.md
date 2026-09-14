# 0018: English as the primary documentation edition

**English** | [Chinese](../zh-CN/04-decisions/2026-09-14-0018-english-primary-documentation.md)

- Status: accepted
- Date: 2026-09-14
- Related: [Documentation standards](../03-engineering/09-documentation-standards.md)

## Context

[Fact] The repository previously used Chinese for its project README, design documents, ADRs, and provider guides, and English for code and published crate documentation (the previous documentation standards, section 1).

## Decision

[Decision] English is the primary edition at `README.md` and `docs/`. Keep a complete, separate Chinese edition at `README.zh-CN.md` and `docs/zh-CN/`, with matching document names, section structure, ADR numbers, and pending-verification IDs. English is authoritative if the editions disagree. This gives English readers a consistent default entry point while preserving Chinese documentation.

[Decision] Each edition links to its own translated chapters. Language switches are explicit. Source code, fixtures, assets, and generated `docs/api/*.json` remain shared to avoid duplicating implementation artifacts.

[Decision] Use `[Fact]`, `[Decision]`, and `[Pending verification]` in English, with the corresponding Chinese labels in the Chinese edition. Update both editions in the same change when a documented contract, decision, or verification result changes; preserve sources, dates, and validation limits.

## Alternatives

- [Decision] Keeping English under an optional `docs/en/` directory was rejected because English must be the default edition.
- [Decision] Interleaving both languages on every page was rejected because each edition must be independently readable.

## Consequences

[Decision] Update the documentation standards, contributor and agent guidance, indexes, and documentation lint together. Translation does not change SDK behavior, public APIs, dependency versions, or the status of existing verification items.

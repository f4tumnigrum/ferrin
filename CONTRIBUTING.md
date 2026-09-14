# Contributing to Ferrin

The primary [engineering rules](docs/03-engineering/09-documentation-standards.md)
are in English. The independent [Chinese edition](docs/zh-CN/README.md) has the
same chapter structure. English takes precedence when editions disagree.
This file is the short version.

## Setup

```sh
rustup show                         # picks up rust-toolchain.toml (1.98.1)
cargo binstall --locked cargo-nextest cargo-deny cargo-shear cargo-insta \
    cargo-hack cargo-semver-checks cargo-llvm-cov typos-cli just
just check-all
```

## Workflow

- One logical change per PR; keep changes under 800 lines (500 for complex
  logic). Split larger work into reviewable stages.
- Commit messages follow Conventional Commits with the crate short name as
  scope: `feat(core): ...`, `fix(openai): ...`, `docs: ...`.
- Run `just fmt`, `just clippy`, `just test`, `just doctest`, `just doc` and
  `just api-check` before pushing (`just check-all` runs every gate); CI runs
  the same commands plus `cargo deny`, `cargo shear`, `cargo hack`,
  `cargo package` and typos.
- New public API needs rustdoc with `# Errors` / `# Examples` sections.
- Architectural changes need an ADR in `docs/04-decisions/` (see
  `docs/03-engineering/07-adr-process.md`).
- Update matching English and Chinese pages together, keeping internal chapter
  links within each edition and preserving sources, dates, ADR/PV IDs and status.
  Use `[Fact]`, `[Decision]` or `[Pending verification]` in English and the
  corresponding Chinese labels in the Chinese edition. Register pending IDs in
  each edition's appendix. Run `just docs-lint` and `just typos` for documentation
  changes (ADR 0018).

## Tests

- Tests never sit next to source files: no `*_tests.rs` under `src/` and no
  `#[cfg(test)] mod tests` inside implementation files.
- Tests live in `tests/suite/*.rs`, aggregated by `tests/all.rs`. Logic that
  is only reachable with crate-internal visibility is tested from `src/tests/`.
- Provider tests replay fixtures; live tests are `#[ignore]` and keyed on
  environment variables.

## Licensing

Contributions are licensed under Apache-2.0, matching the project. Code
derived from another project must be attributed in `NOTICE`, in the crate
documentation of the affected crate and in the module documentation of the
affected file (ADR 0017).

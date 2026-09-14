# Documentation standards

**English** | [Chinese](../zh-CN/03-engineering/09-documentation-standards.md)

## 1. Document types

| Type | Location | Language | Audience |
| --- | --- | --- | --- |
| Design documents (this collection) | `docs/00-05` | English prose and code | Maintainers |
| ADRs | `docs/04-decisions/` | English prose and code | Maintainers |
| Provider guides | `docs/providers/<name>.md` | English prose and code | Maintainers and users |
| API documentation | rustdoc (source comments) | English | Users |
| Project README | `README.md` | English prose and code | Users (repository homepage: features, quick start, usage, status) |
| Design documentation index | `docs/README.md` | English | Maintainers (contents and conventions) |
| Crate READMEs | `crates/*/README.md` | English | Users (crates.io pages) |
| Examples | `examples/` | English code and comments | Users |
| Changelogs | `CHANGELOG.md` | English | Users |

[Decision] English is the primary edition at `README.md` and `docs/`. A complete, independent Chinese edition lives at `README.zh-CN.md` and `docs/zh-CN/`. English takes precedence if the editions disagree. Each edition links to its own chapters, with explicit language switches; source code, assets, fixtures, and generated `docs/api/*.json` remain shared. This replaces the Chinese-only design-document policy ([ADR 0018](../04-decisions/2026-09-14-0018-english-primary-documentation.md)) so the default repository entry points and published crate documentation use the same language.

## 2. Writing rules

- Mark every statement with its source category: `[Fact]` (source path), `[Decision]` (rationale), or `[Pending verification]` (registered in the appendix). Use the corresponding Chinese labels in the Chinese edition.
- Do not include conversation history, evaluations of requests, or aspirational slogans.
- Distinguish design targets from implemented and verified capabilities. Do not describe Ferrin as supporting or implementing a capability before its implementation is complete.
- Keep one topic per paragraph; use tables for comparisons and code blocks for signatures and examples.
- Format identifiers, file paths, and commands as code.
- Cite locatable sources: URLs and sections for official documentation or specifications, versions and module paths for dependency crates, and relative paths and function names for repository code.

## 3. Synchronization obligations

| Change | Documentation to update in both editions |
| --- | --- |
| Public API addition/change | rustdoc, `docs/02-api/02-api-reference.md`, `CHANGELOG.md` |
| Crate addition/removal | `docs/01-architecture/02-crates.md`, `docs/03-engineering/02-workspace-layout.md`, publish order |
| Dependency version change | Verification record in `docs/03-engineering/01-toolchain-and-dependencies.md` |
| Decision change | New ADR and the corresponding design-document paragraphs |
| Pending item closed | Replace its original marker with `[Fact]` or `[Decision]`, record the result in the text, and update the appendix |
| Provider capability change | Capability matrix in `docs/providers/<name>.md` |

[Decision] Update matching Chinese pages in the same change, retaining section structure, ADR numbers, PV IDs, sources, dates, and verification limits. CI documentation checks validate Markdown links and pending-item registration in each edition; `just doc` separately checks rustdoc warnings. See [ADR 0018](../04-decisions/2026-09-14-0018-english-primary-documentation.md).

## 4. rustdoc rules

- Crate documentation (`//!` in `lib.rs`) includes a one-sentence purpose, its architectural layer, a minimal example, and a feature list generated from `Cargo.toml` with `document-features` 0.2.12.
- Public item structure: summary → details → `# Errors` → `# Panics` → `# Examples`.
- Examples must compile. Use `MockLanguageModel` or `no_run` for examples that require the network.
- Annotate feature-gated items with `#[doc(cfg(feature = "..."))]` under `docsrs` cfg.
- Link to repository documentation instead of repeating design background in rustdoc.

## 5. Provider guide template

```markdown
# <Provider>

## Capability matrix
| Capability | Support | Notes |
| Language model (generation/streaming) | Yes | ... |
| Tool calling / provider tools | Yes | web_search, ... |
| Structured output | Yes | strict mode restrictions ... |
| Reasoning | Yes | effort mapping |
| Embeddings / images / speech / transcription / reranking / video | ... |
| Files / skills / batches / realtime | ... |

## Settings and environment variables
## Provider options (provider_options["<key>"])
## Provider metadata (provider_metadata["<key>"])
## Known limitations and warnings
## Fixture inventory
```

## 6. Diagrams

- Use Mermaid for architecture diagrams and include a textual explanation.
- Use sequence diagrams for interactions between components (approval round trips, streaming pipelines).

## 7. Terminology

- Follow the [Glossary](../00-overview/02-glossary.md); update it before introducing a term.
- Use English terminology and identifiers consistently. In the Chinese edition, introduce the English identifier on first use; subsequent mentions may use the Chinese term or identifier.

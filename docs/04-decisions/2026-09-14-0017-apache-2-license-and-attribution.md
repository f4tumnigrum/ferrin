# 0017: Apache-2.0 licensing and attribution

**English** | [Chinese](../zh-CN/04-decisions/2026-09-14-0017-apache-2-license-and-attribution.md)

- Status: accepted
- Date: 2026-09-14
- Related: [Workspace layout](../03-engineering/02-workspace-layout.md), sections 1/2; root `LICENSE`/`NOTICE` and README Acknowledgments

## Context

The workspace originally used `MIT OR Apache-2.0` with separate license files.

[Fact] Design and selected modified Rust ports derive from Vercel AI SDK, including generation, providers, partial JSON, and pruning. Its license is Apache-2.0, copyright Vercel, Inc.; the inspected checkout had no NOTICE on 2026-09-14.

[Fact] Apache section 4 requires license copies, prominent modification notices, retention of copyright/patent/trademark/attribution, and any upstream NOTICE contents.

## Decision

1. Set `Apache-2.0` only, remove `LICENSE-MIT`, and rename `LICENSE-APACHE` to `LICENSE`.
2. Add root `NOTICE` with Ferrin copyright, Vercel-derived crate inventory, Codex engineering acknowledgments, and non-affiliation.
3. Copy `LICENSE`/`NOTICE` into every published crate.
4. Add crate and module rustdoc attribution for derived code, including future additions.
5. Add README acknowledgments, update contribution licensing, and describe the project as independent with upstream-inspired design and ports.

## Rationale

- MIT alone does not preserve Apache patent/modification/notice obligations; a single upstream-compatible license avoids misleading downstream users.
- Apache-2.0 is accepted in Rust and already allowlisted; obligations match the old Apache option.
- Cargo packages only crate-local files, requiring local copies.

## Alternatives

- Reject split per-file licenses because the crate `license` field cannot express the distinction clearly.
- Reject independent rewrites due to cost and difficulty establishing independence after reading upstream code.
- README acknowledgments alone do not meet section 4 requirements.

## Consequences

- Downstream use is Apache-2.0 only; its GPLv2 incompatibility has no practical impact on the intended audience.
- Maintain two copied files in each of 15 crates when license/notice changes.
- Design docs continue citing primary sources; upstream names belong in `NOTICE`, acknowledgments, scope, this ADR, and relevant rustdoc.

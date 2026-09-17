# 0026: Behavioral parity with the reference SDK

**English** | [Chinese](../zh-CN/04-decisions/2026-09-17-0026-reference-sdk-parity.md)

- Status: proposed
- Date: 2026-09-17
- Related: ADRs 0003, 0005, 0012, 0013, 0020, 0021; [Module parity review](../05-appendix/03-reference-parity.md)

## Context

[Fact] Ferrin's Agent implementation and the local Vercel AI SDK checkout at commit `6c6c221` share a tool-loop architecture but differ in named tool contexts, dynamic provider-tool parsing, call preparation, callback scheduling and stream consumption (sources: `ferrin-core/src/agent`, `generate_text`, `stream_text`; reference `packages/ai/src/agent` and `generate-text`, inspected 2026-09-17).

[Decision] Use that fixed reference revision to review every existing Ferrin module. The boundary is the existing eighteen crates and their implemented API families. New providers, frontend protocol modules, Code Mode, Harness and Workflow are separate scope; adding them is not required to establish parity of existing modules.

## Decision

[Decision] Match supported inputs, defaults, preparation precedence, state transitions, provider requests, results, errors and streaming boundaries against the corresponding reference implementation and tests. Language-specific spelling and Rust ownership are expressed idiomatically; any remaining observable difference must be recorded explicitly. A matching method name or a passing pre-existing test suite is insufficient evidence of parity.

[Decision] Select tool context by tool name before schema validation and preserve selected values when no context schema is configured. Accept undeclared provider-executed dynamic calls independently of whether other tools are registered, including input refinement. Keep ordinary unknown local-tool rejection and caller restrictions.

[Decision] Extend call preparation to the reference configuration surface and offer optional call-options schema validation. Explicit per-call timeout takes final precedence. Expose initial instructions and the configured model to step preparation, and allow a sandbox override for one step. Lifecycle callbacks settle concurrently; one callback failure must not prevent other callbacks from completing.

[Decision] Make final streaming-result consumption actively drive the call and support independently consumed event views without detached background tasks. Preserve owner cancellation; document buffering introduced by a lagging view. This revises the consumption contract in ADR 0005 without making an unpolled Rust future execute spontaneously.

[Decision] Align configured approval callbacks and policy adapters with the reference distinction between not-applicable, default decisions and observation-only shadow evaluation. Existing signature validation, secret redaction and secure URL policy remain enforced.

## Verification

[Decision] Track each module's inspected reference paths, concrete differences and regression evidence in the module parity review. Mark a surface verified only after the relevant new regression checks pass. Keep local protocol tests distinct from credentialed provider verification under PV-031.

[Decision] Update bilingual contracts, migration notes, crate changelogs and generated API summaries with implementation changes. This ADR records the authorized implementation direction and remains proposed pending the repository's maintainer review process; it does not assert that all modules have already been aligned.

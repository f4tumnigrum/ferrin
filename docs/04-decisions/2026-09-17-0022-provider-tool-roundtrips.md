# 0022: Complete provider tool roundtrips and caller bindings

**English** | [Chinese](../zh-CN/04-decisions/2026-09-17-0022-provider-tool-roundtrips.md)

- Status: proposed
- Date: 2026-09-17
- Related: [Tool system](../01-architecture/06-tool-system.md), [OpenAI](../providers/openai.md), [Anthropic](../providers/anthropic.md)

## Context

[Fact] The local AI SDK Responses adapter maps hosted programs, tool search and shell output, and expands undeclared `parallel` wrappers. Its programmatic and Anthropic code-execution factories bind provider callers and permit deferred results. Source: `packages/openai/src/responses/` and `packages/anthropic/src/tool/`, inspected on 2026-09-17.

## Decision

[Decision] Complete these paths using the existing Ferrin provider tool and caller contracts. Preserve provider item IDs, program fingerprints and caller identity when replaying calls and results. Hosted shell and server tool search are provider-executed; client tool search and local shell remain application-executed. Programmatic and modern Anthropic code-execution factories enable deferred results because a client callee may finish on a later step.

[Decision] Expand an undeclared `parallel` wrapper only when every nested recipient names a declared function and every parameter value is an object. Preserve wrapper identity and child ordering in provider metadata so stored replay submits one original call and one combined result. Invalid or explicitly declared wrappers remain ordinary calls.

[Decision] Use deterministic protocol regressions for generation, streaming, replay and caller preparation. These checks establish adapter behavior only; official live API verification remains under PV-031.

# 0014: Compatible model families and Responses

**English** | [Chinese](../zh-CN/04-decisions/2026-09-13-0014-openai-compatible-model-families.md)

- Status: accepted
- Date: 2026-09-13
- Related: [Provider guide](../01-architecture/17-provider-implementation-guide.md), sections 6/8/11; [Crates](../01-architecture/02-crates.md); [Compatible endpoints](../providers/openai-compatible.md)

## Context

The guide retained two `OpenAiConfig` compatibility flags but also promised Responses passthrough in the compatible crate. Implementation confirmed its scope is only Chat/Completion/Embedding/Image; those flags concern dedicated Responses adapters such as Azure/Mantle. Record removal of the inapplicable clause through the [ADR process](../03-engineering/07-adr-process.md).

## Decision

1. Compatible provides only chat/completion/embedding/image, without Responses.
2. Use `ferrin-openai` with base_url/name for compatible Responses; keep its two flags.
3. Withdraw the passthrough clause with a dated ADR reference.

## Rationale

- OpenAI already implements Responses and configurable endpoints; a second implementation duplicates conversion.
- Reusing OpenAI would add an unplanned provider-to-provider dependency and all its dependencies.
- Most endpoints claim Chat compatibility; rare Responses endpoints usually need specialized adapters.

## Alternatives

- Reject re-exporting OpenAI Responses because it changes dependency direction/surface.
- Reject independent Responses code because it duplicates event mapping.

## Consequences

- Correct guide exported type names, annotate section 8, and record implementation in section 11.
- Facade compatible features/re-exports remain unchanged.

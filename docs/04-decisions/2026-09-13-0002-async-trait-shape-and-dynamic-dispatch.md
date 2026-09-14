# 0002: Async traits and dynamic dispatch

**English** | [Chinese](../zh-CN/04-decisions/2026-09-13-0002-async-trait-shape-and-dynamic-dispatch.md)

- Status: accepted
- Date: 2026-09-13
- Related: [Provider specification](../01-architecture/04-provider-spec.md), [Overall architecture](../01-architecture/01-overall-architecture.md), section 5

## Context

[Fact] Middleware wraps models and registries resolve arbitrary providers by ID; both need dynamic cross-provider composition.

[Fact] Since Rust 1.75, traits can return `impl Future<Output = T> + Send` without `async-trait`, but such traits are not object-safe.

RPITIT traits remain non-object-safe in Rust 1.98.

## Decision

Use RPITIT specification traits with handwritten object-safe Dyn traits returning `BoxFuture` and blanket implementations. The core holds `Arc<dyn Dyn*>`.

## Rationale

- Native `async fn` implementations without macros.
- Boxing occurs at core boundaries, once per model call, negligible beside network latency.
- Explicit `Send` matches workspace conventions.

## Alternatives

- async_trait is simple/object-safe but boxes every method and hides `Send` in expansion.
- Direct `BoxFuture` traits require manual Box::pin and lose convenient `Send` inference.
- Dynosaur: [Fact] PV-001 verified 0.3.1 Send/static compatibility, but generated unsized structs need explicit `new_arc`/`new_box` and `?Sized`, adding a 0.x macro to spec. [Decision] Retain ordinary handwritten Dyn traits ([Overall architecture](../01-architecture/01-overall-architecture.md), section 5).

## Consequences

- Keep approximately 12 adapter traits/implementations synchronized with specification signatures.
- Model aliases use `Arc<dyn DynLanguageModel>` and equivalents.

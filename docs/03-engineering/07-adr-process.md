# Architecture decision record process

**English** | [Chinese](../zh-CN/03-engineering/07-adr-process.md)

## 1. Purpose

Architecture decision records (ADRs) preserve decisions and rationale with lasting effects on project structure, public contracts, or engineering processes, so future maintainers understand why as well as what.

[Decision] ADR filenames start with a date, use status `proposed`, `accepted`, `rejected`, `deprecated`, or `superseded`, and are indexed in `docs/04-decisions/README.md`. Dates provide chronological ordering; status keeps `superseded` decisions available without misleading readers.

## 2. When an ADR is required

- Adding or removing a crate, or changing dependency direction between crates.
- Breaking changes to public types or traits in `ferrin-spec`.
- Global public API conventions (builders, error model, serialization format).
- Introducing a core external dependency (HTTP client, schema library, runtime).
- Designing or changing security mechanisms (signatures, URL policy, secret handling).
- Major engineering process changes (CI gates, release process, MSRV policy).

Local implementation choices (algorithms, data structures, internal module organization) do not require an ADR; document them in code comments and the PR description.

## 3. File conventions

- Location: `docs/04-decisions/`, with matching Chinese translations in `docs/zh-CN/04-decisions/`.
- Filename: `YYYY-MM-DD-NNNN-<kebab-case-title>.md`, where `NNNN` is a four-digit increasing number.
- Language: English in the primary edition; Chinese prose in the independent Chinese edition. Code and identifiers use English in both ([ADR 0018](../04-decisions/2026-09-14-0018-english-primary-documentation.md)).
- Status: `proposed` → `accepted` or `rejected`; `accepted` may become `deprecated` or `superseded by NNNN`.

## 4. Template

```markdown
# NNNN: <Title>

- Status: proposed | accepted | rejected | deprecated | superseded by NNNN
- Date: YYYY-MM-DD
- Related: <ADRs, issues, PRs>

## Context

<Problem, constraints, relevant facts with sources>

## Decision

<The decision, stated affirmatively>

## Rationale

<Technical reasons and comparison with alternatives>

## Alternatives

- <Option A>: <Why it was rejected>
- <Option B>: <Why it was rejected>

## Consequences

<Effects on code, APIs, dependencies, and process; follow-up work; pending items>
```

## 5. Workflow

1. Submit an ADR PR with status `proposed` (it may be separate from the implementation PR).
2. At least two maintainers review it; all active maintainers must be aware of ADRs affecting `ferrin-spec`.
3. On agreement, set status to `accepted` and merge; if `rejected`, set `rejected` and retain the file.
4. Reference the ADR number in the implementation PR.
5. When replacing a decision, cite the old ADR in the new one and set the old status to `superseded by`.
6. Update `docs/04-decisions/README.md` and its Chinese counterpart in the same PR as the ADR.

## 6. Relationship to this documentation

The initial `[Decision]` entries in this collection were recorded as ADRs 0001–0012 (see the [ADR index](../04-decisions/README.md)). Subsequent changes to an existing documented decision must first follow the ADR process.

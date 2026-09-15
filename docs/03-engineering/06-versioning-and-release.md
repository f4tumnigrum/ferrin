# Versioning and release

**English** | [Chinese](../zh-CN/03-engineering/06-versioning-and-release.md)

## 1. Semantic versioning

- Follow Cargo SemVer: in `0.y.z`, minor increments may break compatibility and patches are compatible; use standard semantics after `1.0`.
- Use semver-checks plus manual review for uncovered cases: non_exhaustive variant additions are compatible, but changed defaults or serialization formats are breaking.
- Major upgrades of allowlisted public third-party types are breaking ([API principles](../02-api/01-api-design-principles.md), section 4).

## 2. Version coordination

| Changed crate | Required coordination |
| --- | --- |
| Breaking `ferrin-spec` | Minor upgrades for providers, tool, message, core, MCP, testing, facade |
| Compatible `ferrin-spec` | None |
| Any `ferrin-core` change | Facade follows |
| Provider crate | Own version; update facade dependency |

[Decision] During `0.y`, all crates share `workspace.package.version` and increment together, avoiding an early compatibility matrix. Switch to independent versions at `1.0` and update this policy then.

## 3. MSRV policy

- Keep `rust-version` aligned with the toolchain's minor version, currently 1.98.
- Raise MSRV only in minor `0.y` or major releases, documenting it.
- Do not promise support more than two minor releases behind current stable.

## 4. Deprecation

- Use deprecated since/note attributes and retain APIs for at least one minor release before removal.
- Avoid alias chains such as old experimental names; point directly to replacements.

## 5. Changelogs

- Each crate follows Keep a Changelog categories `Added`/`Changed`/`Deprecated`/`Removed`/`Fixed`/`Security`.
- Commit entries with code. Release PRs turn `Unreleased` into dated version sections; the workflow validates tag/version and every publishable changelog, permitting `No user-facing changes` for internal refactors.
- [Decision] (PV-025) Use `git-cliff` 2.14.1 via `cliff.toml`/just changelog with crate include paths; release PRs update the shared version and workflow publishes in computed order. Do not use `release-plz`: its `version_group` affects only changed packages, conflicting with all-crate increments, and its changelog engine is already `git-cliff`.

## 6. Release process

1. Release PR updates shared version, crate changelogs, and version references in both document editions; semver and `package` CI pass.
2. Merge and tag `v0.y.z`.
3. In the `release` environment with registry token, validate tag/version/changelogs, then publish all not-yet-indexed crates in one locked multi-package invocation. Cargo orders uploads, waits for index visibility, and uses the publication set for verification builds. On new-crate HTTP 429, wait until the server's retry time and retry remaining crates, up to 40 rounds. Reruns skip published versions.
4. Create GitHub Release notes from the dated root changelog section and attach the complete root changelog.
5. Verify all-feature docs.rs builds for every crate.

[Decision] First release on 2026-09-14 uses an API token and contents-write permission. Trusted Publishing requires existing crates and configured repository trust, unavailable for initial publication. A later switch to crates-io-auth-action would need id-token-write and a policy update.

[Fact] Local mirror configuration prevented packaging unpublished internal dependencies because Cargo multi-`package` overlays apply only to crates.io. A temporary mirror-free `CARGO_HOME` invoked outside the repository packaged and verified all 15 crates in about six minutes on 2026-09-14. A separate publish dry-run was unnecessary because packaging covered its verification steps; actual upload remained release-only. CI runs the same `package` check without a mirror.

Topological order: spec → schema → message → provider utilities → tool → macros → providers/MCP → core → OTel/testing → facade.

[Fact] Initial individual publishing failed on compatible's versioned testing dev dependency, still unpublished. Macros/spec/message/provider-util/schema had already uploaded. An exact-name `ferrin` token scope denied other crates with 403; an unverified account email caused 400.

[Decision] Switch to one multi-`package` publish and make workspace `ferrin-testing` `path`-only. Cargo omits unversioned `path` dev dependencies from published manifests, eliminating the forward publication dependency; testing itself still publishes for downstream users. Enable deny allow-wildcard-paths accordingly ([CI](05-ci-and-quality-gates.md), section 3).

[Fact] New-crate rate limiting allowed five initial uploads, then one about ten minutes later before 429 requested retry after 2026-09-14 07:01 GMT. Existing-crate versions have a separate, looser limit; this motivated server-timed retries (official crates.io rate-limits documentation).

[Fact] Run 34815430254, tag `v0.1.0` at 00e9b61, completed after ten attempts over ~95 minutes. Each retry uploaded one new crate, with release times 07:01, 07:11, …, 08:21 GMT and waits 184–595 s parsed on Ubuntu. All 15 became visible; GitHub Release was created at 08:21:33Z. Every docs.rs status.json reported `doc_status` `true`. The observed new-crate quota replenished roughly once per ten minutes; official documentation does not publish that exact number. Future existing-crate versions do not use this new-crate limit.

## 7. Support policy

- Publish fixes for the latest minor version only.
- Security patches may also target the preceding minor version.

## 8. Specification evolution

Breaking spec changes (required fields, trait signatures, serialization) require:

1. A `proposed` ADR with motivation, affected adapters, and migration steps.
2. Updated testing contracts and mocks first.
3. All first-party adapters updated in the same PR or release batch.
4. SPEC-prefixed `Changed` entries in changelogs.

[Decision] Crate versions carry specification evolution without runtime parallel interfaces or upgrade adapters ([ADR 0011](../04-decisions/2026-09-13-0011-spec-versioning-by-crate-version.md)); this warrants stronger review gates ([CI](05-ci-and-quality-gates.md), section 2, item 11).

## 9. Release 0.1.1 (2026-09-15)

[Fact] Workspace manifests and versioned internal dependencies use 0.1.1. The root and all 15 crate changelogs record its changes under `[0.1.1] - 2026-09-15`, retaining empty `Unreleased` sections for future work (source: `Cargo.toml`, `Cargo.lock`, and crate changelogs). These release files do not establish registry publication success; that requires a verified release-workflow and registry result.

[Decision] The maintainer explicitly selected and authorized publication of 0.1.1 after the breaking schema APIs in [ADR 0019](../04-decisions/2026-09-15-0019-fallible-schema-transforms.md) were explained. This release is an exception to section 1's compatible-patch rule; its version number must not be interpreted as backward API compatibility with 0.1.0. Callers must propagate or handle the new `Result` values from `Schema::transformed`, `SchemaTransform::apply`/`applied`, and `to_openai_strict`. The general versioning policy remains in effect for subsequent releases.

[Fact] Preparation commit `6b6d88b` passed all 14 jobs of [CI run 34944988012](https://github.com/f4tumnigrum/ferrin/actions/runs/34944988012), including tests on three platforms and package verification. This run predates the dated release-documentation changes and does not verify registry uploads or docs.rs builds for 0.1.1.

[Fact] [Release run 34946615509](https://github.com/f4tumnigrum/ferrin/actions/runs/34946615509) completed for tag `v0.1.1` at `a7cad68f`, which passed all 14 jobs of [CI run 34946103620](https://github.com/f4tumnigrum/ferrin/actions/runs/34946103620). The official crates.io API confirms all 15 unyanked 0.1.1 versions, and every docs.rs `status.json` reports `doc_status: true` (verified 2026-09-15). The [GitHub Release](https://github.com/f4tumnigrum/ferrin/releases/tag/v0.1.1) was published at `2026-09-15T08:26:28Z` and includes the compatibility notice above.

## 10. Release 0.1.2 (2026-09-16)

[Fact] The workspace manifests, lockfile, API snapshots and all 16 crate changelogs prepare version 0.1.2 (source: release files). `ferrin-policy` is included for its first publication. This preparation does not establish successful registry uploads or docs.rs builds.

[Decision] The maintainer authorized 0.1.2 after review fixes for capability enforcement, tool-choice synchronization and diagnostic redaction. The new model middleware and policy feature APIs are additive; no provider specification fields are added.

[Decision] Historical Claude co-author trailers are removed from the default branch while existing published release tags remain unchanged. Because those tags belong to the original history, release notes are extracted from the dated root changelog instead of inferring a previous tag from ancestry.

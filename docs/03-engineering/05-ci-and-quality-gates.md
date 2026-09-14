# CI and quality gates

**English** | [Chinese](../zh-CN/03-engineering/05-ci-and-quality-gates.md)

## 1. Workflows

[Decision] Separate formatting, Clippy, platform tests, docs, and audits into parallel jobs; pin toolchains and action commit SHAs for clear failures and protection against tag drift.

Ferrin workflows:

| Workflow | Trigger | Jobs |
| --- | --- | --- |
| `ci.yml` | PRs and main pushes | `fmt`, `clippy`, three-platform nextest/doctests, `doc`/API snapshot, `deny`, `shear`, `examples`, `msrv`, `features`, `docs-lint`, `package`, `lockfile` |
| `semver.yml` | PR manifest/crate changes, including label events | Compare with latest `v*` tag reachable from base branch; explain and skip before first release |
| `coverage.yml` | Main pushes and PRs | llvm-cov summary and 14-day lcov artifact |
| `live-tests.yml` | Manual and weekly | Credentialed live tests |
| `bench.yml` | Manual `filter` input | All-feature criterion, 30-day artifacts; no timing gate, Clippy compiles targets; see [Testing](04-testing.md), section 11 |
| `versions.yml` | Weekly/manual | Outdated-dependency and advisory issues |
| `release.yml` | `v*` tags | Publish in dependency order; see [Versioning](06-versioning-and-release.md) |
| `typos.yml` | PR | `typos` |

## 2. Gate checklist

Required conditions for merging (branch protection policy):

[Fact] Before public release, the private GitHub Free repository could not enforce protection. After becoming public on 2026-09-14, `protect-main` forbids deletion/force pushes (section 8). Required checks/reviews remain unenforced while maintainers push directly; enable all CI jobs as required when adopting PRs.

1. Formatting check with Item imports passes.
2. Workspace/all-target/all-feature Clippy with denied warnings passes on Linux.
3. All-feature nextest without fail-fast and separate doctests pass on Linux/macOS/Windows.
4. All-feature rustdoc with warnings denied and docsrs cfg passes; CI elevates `missing_docs` for published crates.
5. All four cargo deny checks pass.
6. `cargo shear --deny-warnings` passes.
7. Each-feature cargo hack without dev dependencies passes.
8. MSRV 1.98.0 locked all-feature check passes; development toolchain remains 1.98.1.
8a. docs_lint validates relative links and pending IDs/registration in each language edition.
9. All examples compile.
10. Jobs leave no generated diffs; `lockfile` job uses `cargo update --workspace --locked` to check manifest consistency.
10a. Package every publishable crate in publish order with --locked, compiling packaged sources matching release contents.
11. At least one maintainer approves; specification public-type changes need two and an ADR.

[Fact] Skeleton `shear` reported 292 unused declarations and temporarily continued on error. After the complete build on 2026-09-14, remaining unused dependencies were removed ([Toolchain](01-toolchain-and-dependencies.md), section 5); `shear` is now blocking and clean.

[Fact] Pre-push CI reproduction on 2026-09-14 caught an MCP doctest unable to convert `url::ParseError` with ?, fixed by adding doctest coverage to `just check-all`. Replacing generate-`lockfile` with workspace locked update avoided failures from newly released transitive patches. macOS cross-checking Windows was blocked by `aws-lc-sys` Windows SDK headers; Windows validation relies on CI.

[Fact] Run 34797869442 passed 12/14 jobs; `doc` failed due to auto-trait ordering, Windows trybuild exceeded 360 s while 653 tests passed. Run 34798946529 skipped Windows trybuild and passed everything except docs, also recording PV-028 parser output. After ordering normalization, run 34799653563 passed all 14. Two coverage runs succeeded despite nonfatal unauthenticated Codecov upload.

## 3. cargo deny configuration

```toml
[graph]
all-features = true

[advisories]
version = 2
yanked = "deny"

[licenses]
version = 2
allow = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-3.0", "Zlib", "MPL-2.0", "MIT-0", "CDLA-Permissive-2.0"]

[bans]
multiple-versions = "warn"
wildcards = "deny"
allow-wildcard-paths = true
deny = [
    { crate = "openssl", reason = "rustls only" },
    { crate = "openssl-sys", reason = "rustls only" },
    { crate = "native-tls", reason = "rustls only" },
    { crate = "async-trait", reason = "use RPITIT + Dyn adapters" },
]

[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

[Fact] `MIT-0` comes from `borrow-or-share` via `jsonschema`/`referencing`/`fluent-uri`; `CDLA-Permissive-2.0` comes from Mozilla root data via the system verifier. Both permissive licenses are allowlisted. [Decision] No `native-tls` feature: all-feature auditing would pull banned OpenSSL; reqwest's default system verifier supplies native trust.

[Fact] Path-only `ferrin-testing` dev `dependencies` triggered cargo-`deny` 0.20.2 wildcard failures in runs 34814996884 (9ac9c5c) and 34815425396 (00e9b61), without affecting other jobs. Official bans configuration docs say `allow-wildcard-paths` exempts path/git dev `dependencies` while still rejecting published normal/build `dependencies`.

[Decision] Enable allow-wildcard-paths while keeping `wildcards` deny. Cargo strips unversioned path dev dependencies from packages, so published manifests contain no wildcard; normal wildcard dependencies remain banned. All four local deny checks passed after correction.

## 4. Platform matrix

| Platform | Runner | Notes |
| --- | --- | --- |
| Linux x86_64 | `ubuntu-24.04` | All jobs |
| macOS arm64 | `macos-15` | test |
| Windows x86_64 | `windows-2025` | Tests including stdio and paths |

## 5. Caching and duration

- SHA-pinned rust-cache caches target and registry.
- `ci-test` reduces binary size.
- Target PR CI under 15 minutes; split test binaries or reduce all-feature combinations if exceeded.

## 6. Security checks

- Deny including advisories runs for every PR/push; weekly versions workflow also opens advisory issues.
- [Decision] (PV-027) Dependabot updates Cargo weekly on Monday, grouping minor/patch changes; verification monthly; Actions weekly/grouped. Native GitHub integration needs no extra app and handles lockfiles/groups; Renovate extras are unnecessary.
- Pin action SHAs and update through Dependabot.
- Only live-tests and release workflows require provider or registry secrets; PR jobs do not.

## 7. Generated-file consistency

Commit generated files and compare regeneration in CI:

- docs/api JSON from api-snapshot aids API review. [Fact] Implemented 2026-09-14; `doc` checks after rustdoc and fails on mismatch. Regenerate before committing API changes; see [Workspace layout](02-workspace-layout.md), section 6.
- Provider docs/options-schema.json files contain option schemas.
- Insta snapshots.

## 8. Repository settings

[Fact] Set and read back through GitHub API on public release day, 2026-09-14:

- Enabled private vulnerability reporting, Dependabot alerts/security updates, secret scanning, and push protection. Non-provider patterns/validity checks are unavailable on this plan. CodeQL uses `default` setup/query suite and detected languages.
- `protect-main` targets the default branch with no bypass actors, forbidding deletion/force pushes. Intentional history rewriting requires disabling it first.
- Actions allow official/verified publishers plus pinned third-party actions: Embark deny, Swatinem cache, crate-ci typos, dtolnay toolchain, obi1kenobi semver, peter-evans issue creation, softprops release, taiki-e installation. Tokens default read-only; all external fork workflows require approval. Update the allowlist when adding actions.
- Squash-only merges use PR title/body, delete merged branches, and suggest updating outdated branches.
- Description/topics are configured; social preview and release/live credentials require web configuration.

[Decision] No bypass actors or required checks yet. Accidental force pushes fail; intentional rewriting can disable the rule temporarily. Required checks conflict with current direct-`main` pushes and will be added with the PR workflow.

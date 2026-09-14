set working-directory := "."

# Format all crates (imports_granularity is nightly-only; passed explicitly).
fmt:
    cargo fmt --all -- --config imports_granularity=Item

fmt-check:
    cargo fmt --all -- --config imports_granularity=Item --check

clippy *args:
    cargo clippy --workspace --all-targets --all-features {{args}} -- -D warnings

fix *args:
    cargo clippy --workspace --all-targets --all-features --fix --allow-dirty {{args}}

test *args:
    RUST_MIN_STACK=8388608 NEXTEST_PROFILE=local cargo nextest run --workspace --all-features --no-fail-fast {{args}}

# Doc examples are not run by nextest.
doctest:
    cargo test --workspace --all-features --doc

doc:
    RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo doc --workspace --no-deps --all-features

# Public API summaries under docs/api/ must match the code (regenerate with `cargo xtask api-snapshot`).
api-check:
    cargo xtask api-snapshot --check

module-size:
    cargo xtask check-module-size

deny:
    cargo deny check

shear:
    cargo shear --deny-warnings

features:
    cargo hack check --workspace --each-feature --no-dev-deps

typos:
    typos

docs-lint:
    python3 scripts/docs_lint.py

# Criterion benchmarks; the HTML report lands in target/criterion/report/index.html.
# Filter by name: `just bench sse`, `just bench end_to_end`.
bench *args:
    RUST_MIN_STACK=8388608 cargo bench --workspace --all-features {{args}}

# Package every publishable crate in dependency order (what `release.yml` publishes).
package:
    cargo package --locked $(cargo xtask publish-order | xargs -n1 printf ' -p %s')

check-all: fmt-check clippy test doctest doc api-check module-size deny shear features typos docs-lint

# Verification prototypes for docs/05-appendix/02-pending-verification.md
verify *args:
    cargo test --manifest-path verification/Cargo.toml --workspace --no-fail-fast -- --nocapture --test-threads=1 {{args}}

changelog crate:
    git cliff --config cliff.toml --include-path "crates/{{crate}}/**" --unreleased

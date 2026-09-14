# verification

Prototype workspace for the pending-verification items in
`../docs/05-appendix/02-pending-verification.md`. Independent from the product
workspace; run with:

```sh
cargo test --workspace -- --nocapture      # facts printed by each prototype
cargo run --release -p pv006-joinset -- 200 64
cargo run --release -p pv008-partial-compare
cargo run --release -p pv020-sleep-reset
```

| Crate | Item |
| --- | --- |
| `pv001-dynosaur` | PV-001 |
| `pv002-data-url` | PV-002 |
| `pv004-schema` | PV-004 |
| `pv005-static-capture` | PV-005 |
| `pv006-joinset` | PV-006 |
| `pv008-partial-compare` | PV-008 |
| `pv009-override` | PV-009 |
| `pv013-error-size` | PV-013 |
| `pv014-otel` | PV-014 |
| `pv015-reqwest` | PV-015 |
| `pv016-header-values` | PV-016 |
| `pv020-sleep-reset` | PV-020 |
| `pv022-crypto` | PV-022 |
| `pv023-api-changes` | PV-023 |
| `pv026-sse-server` | PV-026 |
| `pv028-ipv4-mapped` | PV-028 |

Items without a crate were closed from official documentation or
specifications, or by a recorded decision; see the appendix.

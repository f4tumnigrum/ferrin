# Provider tool schema fixtures

These fixtures were evaluated on 2026-09-17 against the Vercel AI SDK checkout
`6c6c221`, `packages/anthropic/src/tool/*.ts`, using its locked Zod 3.25.76
`zod/v4` export. Each fixture records a valid input/output, the reference
parser's normalized value, and invalid required/type cases. Argument cases
also record the corresponding reference prepare-tools wire conversion.

Factories and schema wrappers were isolated from networking for this run.
These fixtures test schema and request translation, not provider service
behavior. Live verification remains PV-031. The derived definitions retain
Vercel AI SDK Apache-2.0 attribution in the crate NOTICE and schema modules.

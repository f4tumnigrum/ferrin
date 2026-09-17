# ferrin-voyage

Voyage reranking provider for Ferrin. It supports text documents, JSON objects
serialized with a compatibility warning, `top_n`, `returnDocuments` and
`truncation`. Voyage embeddings are outside this adapter's scope.

```rust,no_run
use ferrin_voyage::{VoyageSettings, create_voyage};

# async fn run() -> Result<(), Box<dyn std::error::Error>> {
let voyage = create_voyage(VoyageSettings::default())?;
let result = ferrin_core::rerank(
    voyage.reranking("rerank-2.5"),
    "Rust async runtimes",
    vec!["Tokio is an async runtime.", "A guide to gardening."],
).top_n(1).await?;
# Ok(())
# }
```

Set `VOYAGE_API_KEY`, pass a `SecretString` in settings, or provide an
Authorization header. Credentials are loaded only when a request is made.
Custom base URLs, provider names, headers and HTTP transports are supported.
Use the facade's `voyage` feature for `ferrin::providers::voyage`.

The provider guide is at [`docs/providers/voyage.md`](../../../docs/providers/voyage.md).
Tests use handwritten local fixtures; credentialed Voyage service verification
remains outstanding under PV-031. Licensed under Apache-2.0; see `NOTICE` for
Vercel AI SDK attribution.

# Azure OpenAI (`ferrin-azure`)

**English** | [Chinese](../zh-CN/providers/azure.md)

[Decision] `ferrin-azure` reuses the OpenAI wire models with Azure routing and authentication ([ADR 0025](../04-decisions/2026-09-17-0025-azure-and-voyage-providers.md)). Enable the facade feature `azure`, or depend on the crate directly.

## Capabilities and configuration

[Fact] `create_azure(AzureSettings)` returns `AzureProvider`; its `responses`, `chat`, `completion`, `embedding`, `image`, `speech` and `transcription` methods take an Azure deployment name. `Provider::language_model` selects Responses. `tools()` exposes the OpenAI tool factories. Source: `crates/providers/ferrin-azure/src/provider.rs`.

[Decision] Options and result metadata use the `openai` key; model identities use `azure.<family>`. The underlying model capability rules currently inspect deployment names, as the reference Azure adapter does. Applications should use suitable deployment names for model-specific sampling and image constraints; the SDK does not discover the underlying model from Azure. Azure-specific DeepSeek, realtime, files, skills and batch services are outside this adapter.

[Fact] Authentication is lazy: explicit `api_key` wins over `AZURE_API_KEY`; alternatively `token_provider` obtains an Entra token for every request. Supplying both explicit methods is invalid. `resource_name` or `AZURE_RESOURCE_NAME` constructs `https://<resource>.openai.azure.com/openai`; `base_url` overrides it. Source: `settings.rs`, `provider.rs`, `transport.rs`.

[Decision] `AzureUrlMode::V1` adds `/v1` to unversioned Azure OpenAI hosts, preserves complete `/openai/v1` and custom gateway URLs, and recognizes Foundry project paths. `Deployment` uses `/deployments/<deployment>` and an `api-version` query. The version defaults to `v1`; configure the dated version required by legacy deployments. Invalid deployment names fail before HTTP.

[Decision] Credentials are restricted to the configured origin and API path prefix. Cross-origin URLs, sibling paths, URL credentials, decoded path traversal, backslashes, control characters and ambiguous double encoding receive no Azure credential. Existing download URL validation remains in force. Explicit authentication overrides per-call credential headers; errors and debug output never expose credential values.

[Decision] Token acquisition respects cancellation and request timeouts. Time spent acquiring the token is deducted from the inner transport's response-body deadline. HTTP transcription is wrapped independently so enabling another crate's OpenAI realtime feature cannot activate an unsupported Azure WebSocket path.

## Example

```rust,no_run
use ferrin::azure::{AzureSettings, create_azure};
use ferrin::prelude::*;

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let azure = create_azure(AzureSettings::new("my-resource"))?;
let response = generate_text(azure.responses("my-deployment"))
    .prompt("Explain ownership in Rust.")
    .await?;
# Ok(())
# }
```

## Verification

[Fact] Tests in `crates/providers/ferrin-azure/tests/suite/` cover Responses generation/streaming, embeddings, URL modes and Foundry, per-request Entra refresh, conflicting authentication, invalid input, token failure redaction, cancellation, credential scoping, pinned-address forwarding and total timeout accounting. These are fixture and injected-transport tests, not service verification.

[Pending verification] (PV-031) Record real Azure responses and compare the fixtures; deployment capability inference and tenant-specific endpoint availability are not verified by local tests.

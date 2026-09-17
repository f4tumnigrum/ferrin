# Ferrin Azure OpenAI provider

Azure OpenAI Responses, Chat Completions, Completions, embeddings, images,
speech and transcription using the shared OpenAI model implementations.

```rust,no_run
use ferrin_azure::{create_azure, AzureSettings};
let provider = create_azure(AzureSettings::new("my-resource"))?;
let model = provider.responses("my-deployment");
# Ok::<(), ferrin_spec::error::ProviderError>(())
```

Set `AZURE_API_KEY` (resolved per request), or supply an asynchronous
`token_provider` returning a `SecretString`. Explicit API key and token
provider settings are mutually exclusive. `AZURE_RESOURCE_NAME` is used
when neither a resource nor a base URL is configured. `AzureUrlMode::Deployment`
selects legacy deployment URLs; configure the matching `api_version`.

Model options retain the `openai` namespace. Provider identities use `azure`.
Only Azure OpenAI model endpoints are exposed; other Azure services and
credential acquisition SDKs are application responsibilities. Fixtures verify
protocol translation; live Azure service behavior is not yet verified.

Licensed under Apache-2.0; see LICENSE and NOTICE.

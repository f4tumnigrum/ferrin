use std::sync::Arc;
use std::sync::Mutex;

use ferrin_azure::AzureSettings;
use ferrin_azure::AzureUrlMode;
use ferrin_azure::create_azure;
use ferrin_provider_util::HttpRequest;
use ferrin_provider_util::HttpResponse;
use ferrin_provider_util::HttpTransport;
use ferrin_provider_util::TransportError;
use ferrin_spec::BoxFuture;
use ferrin_spec::CallOptions;
use ferrin_spec::Headers;
use ferrin_spec::LanguageModel;
use ferrin_spec::Prompt;
use pretty_assertions::assert_eq;
use secrecy::SecretString;
use serde_json::json;

#[derive(Default)]
struct Capture(Mutex<Vec<HttpRequest>>);

impl HttpTransport for Capture {
    fn execute(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        self.0.lock().unwrap().push(request);
        Box::pin(async {
            Ok(HttpResponse::from_bytes(http::StatusCode::OK, Headers::new(), json!({"id":"resp_test","created_at":1,"model":"test","output":[],"usage":{"input_tokens":0,"output_tokens":0}}).to_string().into()))
        })
    }
}

#[tokio::test]
async fn azure_hosts_custom_gateways_and_foundry_keep_their_url_contracts() {
    let cases = [
        (
            None,
            AzureUrlMode::V1,
            "https://example.openai.azure.com/openai/v1/responses?api-version=v1",
            false,
        ),
        (
            Some("https://example.openai.azure.com/openai"),
            AzureUrlMode::V1,
            "https://example.openai.azure.com/openai/v1/responses?api-version=v1",
            false,
        ),
        (
            Some("https://example.openai.azure.com/openai/v1/"),
            AzureUrlMode::V1,
            "https://example.openai.azure.com/openai/v1/responses",
            false,
        ),
        (
            Some("https://gateway.example.com/azure/v1"),
            AzureUrlMode::V1,
            "https://gateway.example.com/azure/v1/responses",
            false,
        ),
        (
            Some("https://example.services.ai.azure.com/api/projects/project/openai"),
            AzureUrlMode::V1,
            "https://example.services.ai.azure.com/api/projects/project/openai/v1/responses",
            true,
        ),
        (
            Some("https://example.openai.azure.com/openai"),
            AzureUrlMode::Deployment,
            "https://example.openai.azure.com/openai/deployments/deployment/responses?api-version=v1",
            false,
        ),
    ];
    for (base, mode, expected_url, explicit_message) in cases {
        let transport = Arc::new(Capture::default());
        let provider = create_azure(AzureSettings {
            resource_name: Some("example".to_owned()),
            base_url: base.map(|url| url.parse().unwrap()),
            url_mode: mode,
            api_key: Some(SecretString::from("test-key")),
            transport: Some(transport.clone()),
            ..AzureSettings::default()
        })
        .unwrap();
        let prompt: Prompt =
            vec![ferrin_spec::language_model::prompt::PromptMessage::user_text("hello")];
        provider
            .responses("deployment")
            .do_generate(CallOptions::new(prompt))
            .await
            .unwrap();
        let requests = transport.0.lock().unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body.to_bytes()).unwrap();
        assert_eq!(
            (
                requests[0].url.as_str(),
                requests[0].headers.get_str("api-key"),
                requests[0].headers.get_str("authorization"),
                body["input"][0].get("type").is_some()
            ),
            (expected_url, Some("test-key"), None, explicit_message)
        );
    }
}

#[test]
fn invalid_endpoint_settings_are_rejected_without_echoing_credentials() {
    for base in [
        "ftp://example.com",
        "https://user:secret@example.com",
        "https://example.com?secret=value",
        "https://example.com#fragment",
    ] {
        let result = create_azure(AzureSettings {
            base_url: Some(base.parse().unwrap()),
            ..AzureSettings::default()
        });
        let error = result.unwrap_err();
        assert!(!error.to_string().contains("secret"));
    }
    for name in ["", "a.b", "a/b", "a@b"] {
        assert!(create_azure(AzureSettings::new(name)).is_err());
    }
}

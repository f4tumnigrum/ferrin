//! Shared fixture helpers.

use std::sync::Arc;

use ferrin_spec::ProviderOptions;
use ferrin_spec::reranking_model::RerankDocuments;
use ferrin_spec::reranking_model::RerankOptions;
use ferrin_testing::RecordingTransport;
use ferrin_voyage::VoyageProvider;
use ferrin_voyage::VoyageSettings;
use ferrin_voyage::create_voyage;
use secrecy::SecretString;
use serde_json::Value;
use url::Url;
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::method;
use wiremock::matchers::path;

pub(crate) const BASIC: &str = include_str!("../fixtures/reranking/basic.response.json");
pub(crate) const ERROR: &str = include_str!("../fixtures/reranking/error.response.json");

pub(crate) struct TestProvider {
    pub(crate) server: MockServer,
    pub(crate) provider: VoyageProvider,
    pub(crate) transport: Arc<RecordingTransport>,
}

impl TestProvider {
    pub(crate) async fn start() -> Self {
        let server = MockServer::start().await;
        let transport = Arc::new(RecordingTransport::new(
            ferrin_provider_util::default_transport().unwrap(),
        ));
        let provider = create_voyage(VoyageSettings {
            base_url: Some(Url::parse(&format!("{}/v1/", server.uri())).unwrap()),
            api_key: Some(SecretString::from("voyage-fixture-key")),
            headers: ferrin_spec::Headers::new().with("x-application", "configured"),
            transport: Some(transport.clone()),
            ..VoyageSettings::default()
        })
        .unwrap();
        Self {
            server,
            provider,
            transport,
        }
    }

    pub(crate) async fn mount(&self, value: Value, status: u16) {
        Mock::given(method("POST"))
            .and(path("/v1/rerank"))
            .respond_with(
                ResponseTemplate::new(status)
                    .set_body_json(value)
                    .insert_header("x-request-id", "fixture-rerank"),
            )
            .mount(&self.server)
            .await;
    }
}

pub(crate) fn text_options() -> RerankOptions {
    RerankOptions::new(
        "Rust async runtimes",
        RerankDocuments::Text {
            values: vec![
                "A guide to gardening.".into(),
                "Tokio is an async runtime.".into(),
            ],
        },
    )
}

pub(crate) fn provider_options(key: &str, value: Value) -> ProviderOptions {
    [(key.to_owned(), value.as_object().unwrap().clone())].into()
}

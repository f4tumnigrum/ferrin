//! Server-provided resource URLs obey the secure URL policy.

use std::sync::Arc;
use std::sync::Mutex;

use bytes::Bytes;
use ferrin_provider_util::http::HttpRequest;
use ferrin_provider_util::http::HttpResponse;
use ferrin_provider_util::http::HttpTransport;
use ferrin_provider_util::http::TransportError;
use ferrin_provider_util::secure_url::UrlPolicy;
use ferrin_spec::BoxFuture;
use ferrin_spec::Headers;
use ferrin_spec::error::ProviderError;
use http::StatusCode;
use pretty_assertions::assert_eq;
use secrecy::SecretString;
use serde_json::json;
use url::Url;

use ferrin_anthropic::AnthropicSettings;
use ferrin_anthropic::create_anthropic;
use ferrin_spec::Batch;
use ferrin_spec::batch::BatchOperationOptions;
use futures_util::StreamExt;

struct ProbeTransport {
    target: String,
    requests: Mutex<Vec<HttpRequest>>,
    status: StatusCode,
}

impl ProbeTransport {
    fn new(target: &str) -> Self {
        Self {
            target: target.to_owned(),
            requests: Mutex::new(Vec::new()),
            status: StatusCode::OK,
        }
    }
}

impl HttpTransport for ProbeTransport {
    fn execute(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        Box::pin(async move {
            let first = self.requests.lock().unwrap().is_empty();
            self.requests.lock().unwrap().push(request);
            let (status, headers, body) = if first {
                (
                    StatusCode::OK,
                    Headers::new().with("content-type", "application/json"),
                    json!({"id":"batch-1", "processing_status":"ended", "results_url":self.target})
                        .to_string(),
                )
            } else {
                (
                    self.status,
                    Headers::new()
                        .with("content-type", "application/json")
                        .with("location", "https://127.0.0.1/redirect"),
                    r#"{"custom_id":"item-1","result":{"type":"canceled"}}"#.to_owned(),
                )
            };
            Ok(HttpResponse::from_bytes(status, headers, Bytes::from(body)))
        })
    }
}

async fn perform(
    transport: Arc<ProbeTransport>,
    policy: UrlPolicy,
    base_url: Option<Url>,
) -> Result<(), ProviderError> {
    let settings = AnthropicSettings {
        api_key: Some(SecretString::from("test-key")),
        headers: Headers::new()
            .with("authorization", "Bearer test-token")
            .with("cookie", "session=test-cookie")
            .with("x-custom-key", "test-extra"),
        transport: Some(transport),
        url_policy: policy,
        base_url,
        ..AnthropicSettings::default()
    };
    let provider = create_anthropic(settings).unwrap();
    let mut stream = provider
        .batch()
        .do_get_batch_results(BatchOperationOptions::new("batch-1"))
        .await?;
    while let Some(item) = stream.next().await {
        item?;
    }
    Ok(())
}

#[tokio::test]
async fn unsafe_server_resource_urls_never_reach_the_transport() {
    for target in [
        "http://8.8.8.8/resource",
        "https://127.0.0.1/resource",
        "https://169.254.169.254/resource",
        "https://[::1]/resource",
        "https://user:pass@8.8.8.8/resource",
    ] {
        let transport = Arc::new(ProbeTransport::new(target));
        let error = perform(transport.clone(), UrlPolicy::new(), None)
            .await
            .unwrap_err();
        assert!(
            matches!(error, ProviderError::InvalidResponseData(_)),
            "{error:?}"
        );
        assert_eq!(transport.requests.lock().unwrap().len(), 1);
    }
}

#[tokio::test]
async fn validated_resource_addresses_are_pinned_and_headers_stay_on_authorized_origins() {
    let target = "https://8.8.8.8/resource";
    for (base, policy, credentials) in [
        (None, UrlPolicy::new(), false),
        (
            Some(Url::parse("https://8.8.8.8/v1").unwrap()),
            UrlPolicy::new(),
            true,
        ),
        (
            None,
            UrlPolicy::new().credential_origin(&Url::parse(target).unwrap()),
            true,
        ),
    ] {
        let transport = Arc::new(ProbeTransport::new(target));
        perform(transport.clone(), policy, base).await.unwrap();
        let requests = transport.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let request = &requests[1];
        assert_eq!(
            request.pinned_addresses,
            vec!["8.8.8.8:443".parse::<std::net::SocketAddr>().unwrap()]
        );
        assert_eq!(
            request.headers.get_str("authorization"),
            credentials.then_some("Bearer test-token")
        );
        assert_eq!(
            request.headers.get_str("cookie"),
            credentials.then_some("session=test-cookie")
        );
        assert_eq!(
            request.headers.get_str("x-custom-key"),
            credentials.then_some("test-extra")
        );
        assert_eq!(
            request.headers.get_str("x-api-key"),
            credentials.then_some("test-key")
        );
    }
}

#[tokio::test]
async fn resource_responses_obey_body_limits_and_do_not_follow_redirects() {
    let transport = Arc::new(ProbeTransport::new("https://8.8.8.8/resource"));
    assert!(
        perform(transport.clone(), UrlPolicy::new().max_body_bytes(1), None)
            .await
            .is_err()
    );
    assert_eq!(transport.requests.lock().unwrap().len(), 2);
    let mut redirect = ProbeTransport::new("https://8.8.8.8/resource");
    redirect.status = StatusCode::FOUND;
    let transport = Arc::new(redirect);
    assert!(
        perform(transport.clone(), UrlPolicy::new(), None)
            .await
            .is_err()
    );
    assert_eq!(transport.requests.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn resource_error_responses_obey_the_same_body_limit() {
    for status in [StatusCode::BAD_REQUEST, StatusCode::INTERNAL_SERVER_ERROR] {
        let mut probe = ProbeTransport::new("https://8.8.8.8/resource");
        probe.status = status;
        let transport = Arc::new(probe);
        let error = perform(transport.clone(), UrlPolicy::new().max_body_bytes(1), None)
            .await
            .unwrap_err();
        let ProviderError::ApiCall(error) = error else {
            panic!("expected API body error")
        };
        let cause = error
            .cause
            .as_deref()
            .and_then(|cause| cause.downcast_ref::<TransportError>())
            .unwrap();
        assert_eq!(
            (error.status_code, error.response_body, cause.kind),
            (
                Some(status),
                None,
                ferrin_provider_util::http::TransportErrorKind::BodyTooLarge
            )
        );
        assert_eq!(transport.requests.lock().unwrap().len(), 2);
    }
}

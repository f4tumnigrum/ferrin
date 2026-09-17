//! Credentials and request lifetime at the Azure transport boundary.

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;

use ferrin_azure::AzureSettings;
use ferrin_azure::AzureUrlMode;
use ferrin_azure::create_azure;
use ferrin_azure::token_provider;
use ferrin_provider_util::HttpRequest;
use ferrin_provider_util::HttpResponse;
use ferrin_provider_util::HttpTransport;
use ferrin_provider_util::TransportError;
use ferrin_provider_util::http::TransportErrorKind;
use ferrin_spec::BoxFuture;
use ferrin_spec::Headers;
use ferrin_spec::TranscriptionModel;
use futures_util::FutureExt;
use pretty_assertions::assert_eq;
use secrecy::SecretString;

#[derive(Default)]
struct Capture(Mutex<Vec<HttpRequest>>);

impl HttpTransport for Capture {
    fn execute(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        self.0.lock().unwrap().push(request);
        Box::pin(async {
            Ok(HttpResponse::from_bytes(
                http::StatusCode::OK,
                Headers::new(),
                "{}".into(),
            ))
        })
    }
}

#[tokio::test]
async fn credentials_and_token_refresh_are_scoped_to_exact_origin_and_api_path() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let capture = Arc::new(Capture::default());
    let provider = create_azure(AzureSettings {
        base_url: Some("https://azure.example.com/openai".parse().unwrap()),
        url_mode: AzureUrlMode::Deployment,
        api_version: Some("v-test".to_owned()),
        token_provider: Some(token_provider(move || {
            let count = count.clone();
            async move {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(SecretString::from("azure-token"))
            }
        })),
        transport: Some(capture.clone()),
        ..AzureSettings::default()
    })
    .unwrap();
    let model = provider.responses("main");
    let targets = [
        (
            "https://azure.example.com/openai/deployments/main/responses?keep=1&api-version=stale",
            true,
        ),
        (
            "https://other.example.com/openai/deployments/main/responses",
            false,
        ),
        (
            "http://azure.example.com/openai/deployments/main/responses",
            false,
        ),
        (
            "https://azure.example.com:444/openai/deployments/main/responses",
            false,
        ),
        ("https://azure.example.com/download/result.png", false),
        (
            "https://azure.example.com/openai/deployments/main-other/responses",
            false,
        ),
        (
            "https://azure.example.com/openai/deployments/other/responses",
            false,
        ),
        ("https://azure.example.com/openai/deployments/main", false),
        (
            "https://azure.example.com/openai/deployments/main/%2e%2e%2fadmin",
            false,
        ),
        (
            "https://azure.example.com/openai/deployments/main/%252e%252e%252fadmin",
            false,
        ),
        (
            "https://azure.example.com/openai/deployments/main/%5c..%5cadmin",
            false,
        ),
        (
            "https://user:password@azure.example.com/openai/deployments/main/responses",
            false,
        ),
    ];
    for (target, _) in targets {
        let mut request = HttpRequest::get(target.parse().unwrap());
        request.headers = Headers::new()
            .with("authorization", "Bearer stray")
            .with("api-key", "stray")
            .with("accept", "application/octet-stream");
        request.pinned_addresses = vec!["203.0.113.7:443".parse().unwrap()];
        model.config().transport.execute(request).await.unwrap();
    }
    let actual = capture
        .0
        .lock()
        .unwrap()
        .iter()
        .map(|request| {
            (
                request.url.to_string(),
                request.headers.get_str("authorization").map(str::to_owned),
                request.headers.get_str("api-key").map(str::to_owned),
                request.headers.get_str("accept").map(str::to_owned),
                request.pinned_addresses.clone(),
            )
        })
        .collect::<Vec<_>>();
    let expected=targets.into_iter().map(|(url,authorized)| (
        if authorized {"https://azure.example.com/openai/deployments/main/responses?keep=1&api-version=v-test".to_owned()} else {url.to_owned()},
        authorized.then(|| "Bearer azure-token".to_owned()),None,Some("application/octet-stream".to_owned()),vec!["203.0.113.7:443".parse().unwrap()],
    )).collect::<Vec<_>>();
    assert_eq!(actual, expected);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn api_key_authentication_never_leaks_to_download_requests() {
    let capture = Arc::new(Capture::default());
    let provider = create_azure(AzureSettings {
        base_url: Some("https://azure.example.com/openai/v1".parse().unwrap()),
        api_key: Some(SecretString::from("azure-test-key")),
        transport: Some(capture.clone()),
        ..AzureSettings::default()
    })
    .unwrap();
    let model = provider.responses("main");
    for path in [
        "openai/v1/responses",
        "results/image.png",
        "openai/v10/responses",
    ] {
        let request =
            HttpRequest::get(format!("https://azure.example.com/{path}").parse().unwrap())
                .with_headers(
                    Headers::new()
                        .with("api-key", "unrelated-key")
                        .with("authorization", "Bearer unrelated"),
                );
        model.config().transport.execute(request).await.unwrap();
    }
    assert_eq!(
        capture
            .0
            .lock()
            .unwrap()
            .iter()
            .map(|request| (
                request.headers.get_str("api-key").map(str::to_owned),
                request.headers.get_str("authorization").map(str::to_owned),
            ))
            .collect::<Vec<_>>(),
        vec![
            (Some("azure-test-key".to_owned()), None),
            (None, None),
            (None, None)
        ]
    );
}

#[tokio::test]
async fn cancellation_drops_an_in_progress_token_callback_before_http() {
    let capture = Arc::new(Capture::default());
    let drops = Arc::new(AtomicUsize::new(0));
    struct DropGuard(Arc<AtomicUsize>);
    impl Drop for DropGuard {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let dropped = drops.clone();
    let provider = create_azure(AzureSettings {
        base_url: Some("https://azure.example.com/openai/v1".parse().unwrap()),
        token_provider: Some(token_provider(move || {
            let guard = DropGuard(dropped.clone());
            async move {
                let _guard = guard;
                std::future::pending().await
            }
        })),
        transport: Some(capture.clone()),
        ..AzureSettings::default()
    })
    .unwrap();
    let model = provider.responses("main");
    let request = HttpRequest::get(
        "https://azure.example.com/openai/v1/responses"
            .parse()
            .unwrap(),
    );
    let cancellation = request.cancellation.clone();
    let mut execute = model.config().transport.execute(request);
    assert!(execute.as_mut().now_or_never().is_none());
    cancellation.cancel();
    assert_eq!(
        execute.await.unwrap_err().kind,
        TransportErrorKind::Cancelled
    );
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(capture.0.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn token_acquisition_consumes_the_downstream_response_body_timeout() {
    let capture = Arc::new(Capture::default());
    let (release, wait_release) = tokio::sync::oneshot::channel();
    let receiver = Arc::new(Mutex::new(Some(wait_release)));
    let provider = create_azure(AzureSettings {
        base_url: Some("https://azure.example.com/openai/v1".parse().unwrap()),
        token_provider: Some(token_provider(move || {
            let receiver = receiver.lock().unwrap().take().unwrap();
            async move {
                receiver.await.unwrap();
                Ok(SecretString::from("token"))
            }
        })),
        transport: Some(capture.clone()),
        ..AzureSettings::default()
    })
    .unwrap();
    let model = provider.responses("main");
    let request = HttpRequest::get(
        "https://azure.example.com/openai/v1/responses"
            .parse()
            .unwrap(),
    )
    .with_timeout(Duration::from_secs(10));
    let mut execute = model.config().transport.execute(request);
    assert!(execute.as_mut().now_or_never().is_none());
    tokio::time::advance(Duration::from_secs(6)).await;
    release.send(()).unwrap();
    execute.await.unwrap();
    assert_eq!(
        capture.0.lock().unwrap()[0].timeout,
        Some(Duration::from_secs(4))
    );
}

#[tokio::test]
async fn azure_transcription_never_inherits_openai_websocket_support() {
    let provider = create_azure(AzureSettings {
        base_url: Some("https://azure.example.com/openai/v1".parse().unwrap()),
        ..AzureSettings::default()
    })
    .unwrap();
    assert!(
        !provider
            .transcription("gpt-realtime-whisper")
            .supports_stream()
    );
}

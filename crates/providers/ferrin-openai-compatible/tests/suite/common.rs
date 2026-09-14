//! Shared helpers: a fixture server plus a provider pointing at it.

use std::path::PathBuf;
use std::sync::Arc;

use ferrin_openai_compatible::OpenAiCompatibleProvider;
use ferrin_openai_compatible::OpenAiCompatibleSettings;
use ferrin_openai_compatible::create_openai_compatible;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderOptions;
use ferrin_spec::StreamPart;
use ferrin_spec::language_model::StreamResult;
use ferrin_testing::FixtureServer;
use ferrin_testing::ReceivedRequest;
use ferrin_testing::SequentialIdGenerator;
use ferrin_testing::StreamContractChecker;
use http::Method;
use secrecy::SecretString;

/// API key used against the fixture server (never a real key).
pub(crate) const TEST_API_KEY: &str = "test-key";

/// Provider name of the default test provider.
pub(crate) const PROVIDER_NAME: &str = "example";

/// A provider wired to a local fixture server.
pub(crate) struct TestProvider {
    pub(crate) server: FixtureServer,
    pub(crate) provider: OpenAiCompatibleProvider,
}

impl TestProvider {
    pub(crate) async fn start() -> Self {
        Self::start_with(|settings| settings).await
    }

    pub(crate) async fn start_with(
        customize: impl FnOnce(OpenAiCompatibleSettings) -> OpenAiCompatibleSettings,
    ) -> Self {
        let server = FixtureServer::start().await.unwrap();
        let base_url = server.url().join("v1").unwrap();
        let mut settings = OpenAiCompatibleSettings::new(PROVIDER_NAME, base_url);
        settings.api_key = Some(SecretString::from(TEST_API_KEY.to_owned()));
        settings.id_generator = Some(Arc::new(SequentialIdGenerator::new("id")));
        let provider = create_openai_compatible(customize(settings)).unwrap();
        Self { server, provider }
    }

    /// Mounts a fixture file (`tests/fixtures/<area>/<case>.*`).
    pub(crate) fn mount(&self, method: Method, path: &str, area: &str, case: &str) {
        self.server
            .mount_file(method, path, fixtures_dir(area), case)
            .unwrap();
    }

    /// The single request the server received.
    pub(crate) fn only_request(&self) -> ReceivedRequest {
        let received = self.server.received();
        assert_eq!(received.len(), 1, "expected exactly one request");
        received.into_iter().next().unwrap()
    }
}

/// Directory of the fixtures of `area`.
pub(crate) fn fixtures_dir(area: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(area)
}

/// Provider options under `key`.
pub(crate) fn options_under(key: &str, value: JsonValue) -> ProviderOptions {
    let mut options = ProviderOptions::new();
    let object: JsonObject = serde_json::from_value(value).unwrap();
    options.insert(key.to_owned(), object);
    options
}

/// Provider options under the default provider name.
pub(crate) fn example_options(value: JsonValue) -> ProviderOptions {
    options_under(PROVIDER_NAME, value)
}

/// Drains a stream, asserting the specification contract.
pub(crate) async fn collect_checked(result: StreamResult) -> Vec<StreamPart> {
    let (parts, contract) = StreamContractChecker::check_stream(result).await;
    if let Err(violations) = contract {
        panic!("stream contract violated: {violations:?}\nparts: {parts:#?}");
    }
    parts
}

/// Removes the raw usage object (provider specific, noisy) from parts before
/// snapshotting.
pub(crate) fn without_raw_usage(mut parts: Vec<StreamPart>) -> Vec<StreamPart> {
    for part in &mut parts {
        if let StreamPart::Finish { usage, .. } = part {
            usage.raw = None;
        }
    }
    parts
}

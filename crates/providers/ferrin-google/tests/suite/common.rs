//! Shared helpers: a fixture server plus a provider pointing at it.

use std::path::PathBuf;
use std::sync::Arc;

use ferrin_google::GoogleProvider;
use ferrin_google::GoogleSettings;
use ferrin_google::create_google;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderOptions;
use ferrin_spec::StreamPart;
use ferrin_spec::Warning;
use ferrin_spec::language_model::StreamResult;
use ferrin_testing::Fixture;
use ferrin_testing::FixtureServer;
use ferrin_testing::ReceivedRequest;
use ferrin_testing::SequentialIdGenerator;
use ferrin_testing::StreamContractChecker;
use http::Method;
use secrecy::SecretString;

/// API key used against the fixture server (never a real key).
pub(crate) const TEST_API_KEY: &str = "test-key";

/// A provider wired to a local fixture server.
pub(crate) struct TestProvider {
    pub(crate) server: FixtureServer,
    pub(crate) provider: GoogleProvider,
}

impl TestProvider {
    pub(crate) async fn start() -> Self {
        Self::start_with(|settings| settings).await
    }

    pub(crate) async fn start_with(
        customize: impl FnOnce(GoogleSettings) -> GoogleSettings,
    ) -> Self {
        let server = FixtureServer::start().await.unwrap();
        let base_url = server.url().join("v1beta").unwrap();
        let settings = GoogleSettings {
            url_policy: ferrin_provider_util::secure_url::UrlPolicy::new()
                .allow_http()
                .trust_origin(&base_url),
            base_url: Some(base_url),
            api_key: Some(SecretString::from(TEST_API_KEY.to_owned())),
            id_generator: Some(Arc::new(SequentialIdGenerator::new("id"))),
            ..GoogleSettings::default()
        };
        let provider = create_google(customize(settings)).unwrap();
        Self { server, provider }
    }

    /// Mounts a fixture file (`tests/fixtures/<area>/<case>.*`).
    pub(crate) fn mount(&self, method: Method, path: &str, area: &str, case: &str) {
        self.server
            .mount_file(method, path, fixtures_dir(area), case)
            .unwrap();
    }

    /// Mounts an inline fixture.
    pub(crate) fn mount_fixture(&self, method: Method, path: &str, fixture: Fixture) {
        self.server.mount(method, path, fixture);
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

/// Reads a raw fixture file.
pub(crate) fn fixture_bytes(area: &str, name: &str) -> Vec<u8> {
    std::fs::read(fixtures_dir(area).join(name)).unwrap()
}

/// Provider options under `key`.
pub(crate) fn options_under(key: &str, value: JsonValue) -> ProviderOptions {
    let mut options = ProviderOptions::new();
    let object: JsonObject = serde_json::from_value(value).unwrap();
    options.insert(key.to_owned(), object);
    options
}

/// Provider options under the `google` key.
pub(crate) fn google_options(value: JsonValue) -> ProviderOptions {
    options_under("google", value)
}

/// Feature or setting names of `warnings`.
pub(crate) fn features(warnings: &[Warning]) -> Vec<&str> {
    warnings
        .iter()
        .map(|warning| match warning {
            Warning::Unsupported { feature, .. } | Warning::Compatibility { feature, .. } => {
                feature.as_str()
            }
            Warning::Deprecated { setting, .. } => setting.as_str(),
            Warning::Other { message } => message.as_str(),
            _ => "?",
        })
        .collect()
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

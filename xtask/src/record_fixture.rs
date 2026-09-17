//! `cargo xtask record-fixture`: performs one live provider request described
//! by a `*.scenario.json` file and writes the replay fixture next to it.
//!
//! Secrets never reach the fixture files: response headers pass an allow
//! list, and every file is checked for key patterns before it is written.

pub(crate) mod redact;

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use ferrin_provider_util::HttpRequest;
use ferrin_provider_util::HttpTransport;
use ferrin_provider_util::RequestBody;
use ferrin_provider_util::default_transport;
use ferrin_provider_util::http::read_body;
use ferrin_provider_util::settings::env_var;
use ferrin_spec::Headers;
use ferrin_testing::RecordingTransport;
use ferrin_testing::fixture::encode_events_file;
use ferrin_testing::transport::contains_secret;
use http::Method;
use serde::Deserialize;
use serde::Serialize;
use url::Url;

/// Upper bound for a recorded response body.
const MAX_BODY_BYTES: u64 = 64 * 1024 * 1024;

/// Response headers kept in `<case>.meta.json`.
const HEADER_ALLOW_LIST: &[&str] = &[
    "content-type",
    "x-request-id",
    "request-id",
    "retry-after",
    "retry-after-ms",
    "openai-processing-ms",
    "openai-version",
    "x-ratelimit-limit-requests",
    "x-ratelimit-limit-tokens",
    "x-ratelimit-remaining-requests",
    "x-ratelimit-remaining-tokens",
    "x-ratelimit-reset-requests",
    "x-ratelimit-reset-tokens",
    "anthropic-ratelimit-requests-limit",
    "anthropic-ratelimit-requests-remaining",
    "anthropic-ratelimit-tokens-limit",
    "anthropic-ratelimit-tokens-remaining",
];

/// `tests/fixtures/<case>.scenario.json`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    /// HTTP method (default `POST`).
    method: Option<String>,
    /// Path appended to the base URL, including any query string.
    path: String,
    /// Overrides the provider's default base URL.
    base_url: Option<String>,
    /// Reads a private endpoint from the environment; required when configured.
    base_url_env: Option<String>,
    /// Overrides the environment variable holding the API key.
    api_key_env: Option<String>,
    /// Extra request headers.
    #[serde(default)]
    headers: BTreeMap<String, String>,
    /// JSON request body.
    body: Option<serde_json::Value>,
    /// Whether the response is a `text/event-stream`.
    #[serde(default)]
    stream: bool,
    /// Model id recorded in the metadata.
    model: Option<String>,
    /// JSON pointers removed from JSON responses and individual SSE payloads.
    #[serde(default)]
    redact_response_fields: Vec<String>,
}

/// `<case>.meta.json`.
#[derive(Debug, Serialize)]
struct Meta<'a> {
    status: u16,
    headers: BTreeMap<String, String>,
    recorded_at: String,
    provider: &'a str,
    case: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<&'a str>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    redacted_response_fields: &'a Vec<String>,
}

/// Provider defaults: base URL, key variable and the authentication header.
struct ProviderDefaults {
    base_url: Option<&'static str>,
    api_key_env: Option<&'static str>,
    headers: fn(&str) -> Vec<(&'static str, String)>,
}

fn provider_defaults(provider: &str) -> Result<ProviderDefaults> {
    Ok(match provider {
        "openai" => ProviderDefaults {
            base_url: Some("https://api.openai.com/v1"),
            api_key_env: Some("OPENAI_API_KEY"),
            headers: |key| vec![("authorization", format!("Bearer {key}"))],
        },
        "anthropic" => ProviderDefaults {
            base_url: Some("https://api.anthropic.com/v1"),
            api_key_env: Some("ANTHROPIC_API_KEY"),
            headers: |key| {
                vec![
                    ("x-api-key", key.to_owned()),
                    ("anthropic-version", "2023-06-01".to_owned()),
                ]
            },
        },
        "google" => ProviderDefaults {
            base_url: Some("https://generativelanguage.googleapis.com/v1beta"),
            api_key_env: Some("GOOGLE_GENERATIVE_AI_API_KEY"),
            headers: |key| vec![("x-goog-api-key", key.to_owned())],
        },
        "openai-compatible" => ProviderDefaults {
            base_url: None,
            api_key_env: None,
            headers: |key| vec![("authorization", format!("Bearer {key}"))],
        },
        other => bail!(
            "unknown provider {other:?} (expected openai, anthropic, google or openai-compatible)"
        ),
    })
}

fn fixture_dir(provider: &str) -> Result<PathBuf> {
    let metadata = crate::workspace::metadata_no_deps()?;
    let dir = metadata
        .workspace_root
        .join("crates")
        .join("providers")
        .join(format!("ferrin-{provider}"))
        .join("tests")
        .join("fixtures");
    if !dir.is_dir() {
        bail!("fixture directory {dir} does not exist");
    }
    Ok(dir.into_std_path_buf())
}

fn validate_case(case: &str) -> Result<()> {
    let valid = !case.is_empty()
        && case
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/'))
        && !case.starts_with('/')
        && !case.ends_with('/')
        && !case.contains("//");
    if !valid {
        bail!(
            "case must look like `<area>/<name>` (ascii letters, digits, `-`, `_`), got {case:?}"
        );
    }
    Ok(())
}

/// Splits an SSE body into events (blank-line separated, `\r\n` normalised).
fn split_sse_events(body: &str) -> Vec<String> {
    body.replace("\r\n", "\n")
        .split("\n\n")
        .map(|event| event.trim_end_matches('\n'))
        .filter(|event| !event.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Writes `text` to `path` unless it contains a key pattern or the API key.
fn write_checked(path: &Path, text: &str, api_key: &str) -> Result<()> {
    if contains_secret(text) || (!api_key.is_empty() && text.contains(api_key)) {
        bail!(
            "refusing to write {}: content contains a secret pattern",
            path.display()
        );
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    std::fs::write(path, text).with_context(|| format!("failed to write {}", path.display()))?;
    println!("wrote {}", path.display());
    Ok(())
}

fn pretty(value: &serde_json::Value) -> Result<String> {
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    Ok(text)
}

fn build_request(
    scenario: &Scenario,
    defaults: &ProviderDefaults,
    base_url: &str,
    api_key: &str,
) -> Result<HttpRequest> {
    let method = scenario
        .method
        .as_deref()
        .map_or(Ok(Method::POST), Method::from_bytes_str)?;
    let url = Url::parse(&format!(
        "{}/{}",
        base_url.trim_end_matches('/'),
        scenario.path.trim_start_matches('/')
    ))
    .context("invalid request url")?;
    let mut headers = Headers::new();
    for (name, value) in (defaults.headers)(api_key) {
        headers
            .insert(name, &value)
            .with_context(|| format!("invalid header {name}"))?;
    }
    if scenario.stream {
        headers
            .insert("accept", "text/event-stream")
            .context("invalid accept header")?;
    }
    for (name, value) in &scenario.headers {
        headers
            .insert(name, value)
            .with_context(|| format!("invalid header {name}"))?;
    }
    let mut request = HttpRequest::new(method, url).with_headers(headers);
    if let Some(body) = &scenario.body {
        let bytes = serde_json::to_vec(body).context("failed to encode the request body")?;
        request = request.with_body(RequestBody::json(bytes.into()));
    }
    Ok(request)
}

trait MethodExt {
    fn from_bytes_str(text: &str) -> Result<Method>;
}

impl MethodExt for Method {
    fn from_bytes_str(text: &str) -> Result<Method> {
        Method::from_bytes(text.as_bytes()).with_context(|| format!("invalid method {text:?}"))
    }
}

pub(crate) fn run(provider: &str, case: &str) -> Result<()> {
    validate_case(case)?;
    let defaults = provider_defaults(provider)?;
    let dir = fixture_dir(provider)?;
    let scenario_path = dir.join(format!("{case}.scenario.json"));
    let scenario_text = std::fs::read_to_string(&scenario_path)
        .with_context(|| format!("failed to read {}", scenario_path.display()))?;
    let scenario: Scenario = serde_json::from_str(&scenario_text)
        .with_context(|| format!("invalid scenario {}", scenario_path.display()))?;

    if scenario.base_url.is_some() && scenario.base_url_env.is_some() {
        bail!("scenario cannot set both `base_url` and `base_url_env`");
    }
    let environment_base_url = match &scenario.base_url_env {
        Some(name) => Some(
            env_var(name)
                .filter(|value| !value.trim().is_empty())
                .with_context(|| format!("environment variable {name} is not set"))?,
        ),
        None => None,
    };
    let Some(base_url) = environment_base_url
        .as_deref()
        .or(scenario.base_url.as_deref())
        .or(defaults.base_url)
    else {
        bail!("scenario must set `base_url` for provider {provider}");
    };
    let Some(api_key_env) = scenario.api_key_env.as_deref().or(defaults.api_key_env) else {
        bail!("scenario must set `api_key_env` for provider {provider}");
    };
    let Some(api_key) = env_var(api_key_env).filter(|key| !key.trim().is_empty()) else {
        bail!("environment variable {api_key_env} is not set");
    };

    let request = build_request(&scenario, &defaults, base_url, &api_key)?;
    let inner = default_transport().map_err(|error| anyhow::anyhow!("{}", error.message))?;
    let recording = Arc::new(
        RecordingTransport::new(inner).with_header_allow_list(HEADER_ALLOW_LIST.iter().copied()),
    );

    let runtime = crate::workspace::runtime()?;
    let (status, body) = runtime.block_on(async {
        let response = recording
            .execute(request)
            .await
            .map_err(|error| anyhow::anyhow!("request failed: {}", error.message))?;
        let status = response.status;
        let body = read_body(&response.headers, response.body, MAX_BODY_BYTES)
            .await
            .map_err(|error| anyhow::anyhow!("failed to read the response: {}", error.message))?;
        Ok::<_, anyhow::Error>((status, body))
    })?;
    let recorded = recording
        .last_request()
        .context("the request was not recorded")?;
    let body_text = String::from_utf8(body.to_vec()).context("response body is not UTF-8")?;
    println!("{} {} -> HTTP {}", recorded.method, scenario.path, status);

    let case_path = dir.join(case);
    let request_body = recorded.body_json().ok();
    if let Some(request_body) = &request_body {
        write_checked(
            &case_path.with_extension("request.json"),
            &pretty(request_body)?,
            &api_key,
        )?;
    }
    if scenario.stream {
        let events = split_sse_events(&body_text)
            .iter()
            .map(|event| redact::sse_event(event, &scenario.redact_response_fields))
            .collect::<Result<Vec<_>>>()?;
        if events.is_empty() {
            bail!("the response contained no SSE events");
        }
        write_checked(
            &case_path.with_extension("chunks.txt"),
            &encode_events_file(events.iter().map(String::as_str)),
            &api_key,
        )?;
    } else {
        let mut json: serde_json::Value =
            serde_json::from_str(&body_text).context("response body is not JSON")?;
        redact::json(&mut json, &scenario.redact_response_fields);
        write_checked(
            &case_path.with_extension("response.json"),
            &pretty(&json)?,
            &api_key,
        )?;
    }
    let response_headers: BTreeMap<String, String> = recorded
        .response()
        .headers
        .iter_str()
        .map(|(name, value)| (name.to_owned(), value))
        .collect();
    let meta = Meta {
        status: status.as_u16(),
        headers: response_headers,
        recorded_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        provider,
        case,
        model: scenario.model.as_deref(),
        redacted_response_fields: &scenario.redact_response_fields,
    };
    write_checked(
        &case_path.with_extension("meta.json"),
        &pretty(&serde_json::to_value(&meta)?)?,
        &api_key,
    )?;
    Ok(())
}

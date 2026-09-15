use std::collections::HashMap;

use ferrin_provider_util::base_url::join_path;
use ferrin_provider_util::base_url::parse_base_url;
use ferrin_provider_util::base_url::without_trailing_slash;
use ferrin_provider_util::batch::normalize_batch_request_counts;
use ferrin_provider_util::headers::BLOCKED_DOWNLOAD_HEADERS;
use ferrin_provider_util::headers::is_same_origin;
use ferrin_provider_util::headers::sanitize_download_headers;
use ferrin_provider_util::provider_options::parse_provider_options;
use ferrin_provider_util::provider_reference::resolve_provider_reference;
use ferrin_provider_util::response_metadata::response_metadata;
use ferrin_provider_util::retry::is_retryable_status;
use ferrin_provider_util::retry::retry_after;
use ferrin_provider_util::streaming_tool_call::StreamingToolCallTracker;
use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::Headers;
use ferrin_spec::ProviderOptions;
use ferrin_spec::ProviderReference;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolDefinition;
use ferrin_spec::batch::BatchRequestCounts;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde::Deserialize;
use serde_json::json;
use std::time::Duration;
use url::Url;

#[test]
fn base_url_helpers() {
    let base = parse_base_url("https://api.example.com/v1/").unwrap();
    assert_eq!(base.as_str(), "https://api.example.com/v1");
    assert_eq!(
        join_path(&base, "/chat/completions").as_str(),
        "https://api.example.com/v1/chat/completions"
    );
    let root = without_trailing_slash(Url::parse("https://api.example.com/").unwrap());
    assert_eq!(root.as_str(), "https://api.example.com/");
    assert_eq!(
        join_path(&root, "models").as_str(),
        "https://api.example.com/models"
    );
    assert!(parse_base_url("  ").is_err());
    assert!(parse_base_url("not a url").is_err());
}

#[test]
fn batch_counts_require_consistency() {
    assert_eq!(
        normalize_batch_request_counts(Some(10), Some(2), Some(7), Some(1)),
        Some(BatchRequestCounts {
            total: 10,
            pending: 2,
            completed: 7,
            failed: 1,
        })
    );
    assert_eq!(
        normalize_batch_request_counts(Some(10), Some(2), Some(7), Some(2)),
        None
    );
    assert_eq!(
        normalize_batch_request_counts(None, Some(2), Some(7), Some(1)),
        None
    );
}

#[test]
fn download_headers_are_sanitized() {
    let mut headers = Headers::new()
        .with("authorization", "Bearer x")
        .with("accept", "image/*");
    for name in BLOCKED_DOWNLOAD_HEADERS {
        headers = headers.with(name, "1");
    }
    let clean = sanitize_download_headers(headers);
    assert_eq!(clean.len(), 2);
    assert!(clean.contains("authorization"));
    assert!(is_same_origin(
        &Url::parse("https://a.example/x").unwrap(),
        &Url::parse("https://a.example:443/y").unwrap()
    ));
    assert!(!is_same_origin(
        &Url::parse("https://a.example/x").unwrap(),
        &Url::parse("http://a.example/x").unwrap()
    ));
}

#[derive(Debug, Deserialize, PartialEq)]
struct Options {
    user: String,
    #[serde(default)]
    store: bool,
}

#[test]
fn provider_options_parse_by_key() {
    let mut options = ProviderOptions::new();
    let map = json!({ "user": "u1", "store": true })
        .as_object()
        .unwrap()
        .clone();
    options.insert("openai".to_owned(), map);
    assert_eq!(
        parse_provider_options::<Options>("openai", &options).unwrap(),
        Some(Options {
            user: "u1".to_owned(),
            store: true,
        })
    );
    assert_eq!(
        parse_provider_options::<Options>("anthropic", &options).unwrap(),
        None
    );
    let mut bad = ProviderOptions::new();
    bad.insert(
        "openai".to_owned(),
        json!({ "store": 1 }).as_object().unwrap().clone(),
    );
    let error = parse_provider_options::<Options>("openai", &bad).unwrap_err();
    assert_eq!(error.argument, "provider_options");
    assert!(error.message.contains("invalid openai provider options"));
}

#[test]
fn provider_reference_resolution() {
    let mut reference = ProviderReference::new();
    reference.insert("openai".to_owned(), "file-1".to_owned());
    assert_eq!(
        resolve_provider_reference(&reference, "openai").unwrap(),
        "file-1"
    );
    assert!(resolve_provider_reference(&reference, "google").is_err());
}

#[test]
fn response_metadata_from_unix_seconds() {
    let metadata = response_metadata(
        Some("resp_1".to_owned()),
        Some("gpt-x".to_owned()),
        Some(1_700_000_000),
    );
    assert_eq!(metadata.id, Some("resp_1".to_owned()));
    assert_eq!(metadata.model_id, Some("gpt-x".into()));
    assert_eq!(
        metadata.timestamp.map(|time| time.timestamp()),
        Some(1_700_000_000)
    );
}

#[test]
fn retry_classification_and_retry_after() {
    for status in [408, 409, 429, 500, 503] {
        assert!(is_retryable_status(StatusCode::from_u16(status).unwrap()));
    }
    for status in [400, 401, 404] {
        assert!(!is_retryable_status(StatusCode::from_u16(status).unwrap()));
    }
    assert_eq!(
        retry_after(&Headers::new().with("retry-after", "2")),
        Some(Duration::from_secs(2))
    );
    assert_eq!(
        retry_after(&Headers::new().with("retry-after-ms", "1500")),
        Some(Duration::from_millis(1500))
    );
    assert_eq!(
        retry_after(&Headers::new().with("retry-after", "soon")),
        None
    );
    let future = (chrono::Utc::now() + chrono::Duration::seconds(30)).to_rfc2822();
    let delay = retry_after(&Headers::new().with("retry-after", &future)).unwrap();
    assert!(delay > Duration::from_secs(25) && delay <= Duration::from_secs(30));
}

#[test]
fn tool_name_mapping_round_trips() {
    let tools = vec![ToolDefinition::Provider {
        id: "openai.web_search".to_owned(),
        name: "search".into(),
        args: Default::default(),
    }];
    let names = HashMap::from([("openai.web_search", "web_search")]);
    let mapping = ToolNameMapping::new(&tools, &names);
    assert_eq!(mapping.to_provider_tool_name("search"), "web_search");
    assert_eq!(mapping.to_custom_tool_name("web_search"), "search");
    assert_eq!(mapping.to_custom_tool_name("other"), "other");
    assert!(ToolNameMapping::default().is_empty());
}

#[test]
fn tracker_emits_full_sequence_once() {
    let mut tracker = StreamingToolCallTracker::new();
    let parts = tracker.parts_for_complete_call(
        "call_1".into(),
        "weather".into(),
        "{\"city\":\"Paris\"}".to_owned(),
        false,
    );
    let kinds: Vec<&str> = parts.iter().map(StreamPart::kind_name).collect();
    assert_eq!(
        kinds,
        vec![
            "tool-input-start",
            "tool-input-delta",
            "tool-input-end",
            "tool-call"
        ]
    );
    tracker.start("call_2".into());
    let parts =
        tracker.parts_for_complete_call("call_2".into(), "weather".into(), "{}".to_owned(), true);
    let kinds: Vec<&str> = parts.iter().map(StreamPart::kind_name).collect();
    assert_eq!(kinds, vec!["tool-input-end", "tool-call"]);
}

#[test]
fn retry_after_ignores_unrepresentable_delays() {
    for name in ["retry-after", "retry-after-ms"] {
        for value in [
            "NaN",
            "inf",
            "-inf",
            "-1",
            "1e300",
            "18446744073709551616000",
        ] {
            assert_eq!(retry_after(&Headers::new().with(name, value)), None);
        }
    }
    assert_eq!(
        retry_after(
            &Headers::new()
                .with("retry-after-ms", "1e300")
                .with("retry-after", "2")
        ),
        Some(Duration::from_secs(2))
    );
}

//! EOF must not complete buffered tools or report successful generation.

use ferrin_spec::CallOptions;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::StreamPart;
use ferrin_testing::Fixture;
use http::Method;
use pretty_assertions::assert_eq;

use super::common::TestProvider;
use super::common::collect_checked;
use super::common::fixtures_dir;

#[tokio::test]
async fn preterminal_fixture_boundaries_discard_buffered_tools() {
    for (family, fixture) in [
        ("chat", "text-basic-stream"),
        ("chat", "reasoning-stream"),
        ("chat", "tool-call-stream"),
        ("completion", "text-basic-stream"),
    ] {
        let raw =
            std::fs::read_to_string(fixtures_dir(family).join(format!("{fixture}.chunks.txt")))
                .unwrap();
        let events: Vec<_> = raw.lines().filter(|line| !line.is_empty()).collect();
        let terminal = events
            .iter()
            .position(|line| {
                let value: serde_json::Value =
                    serde_json::from_str(line.strip_prefix("data: ").unwrap()).unwrap();
                value["choices"][0]["finish_reason"].is_string()
            })
            .unwrap();
        for end in 0..=terminal {
            let test = TestProvider::start().await;
            let path = if family == "chat" {
                "/v1/chat/completions"
            } else {
                "/v1/completions"
            };
            test.server.mount(
                Method::POST,
                path,
                Fixture::sse(events[..end].iter().copied()),
            );
            let options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
            let result = if family == "chat" {
                test.provider
                    .chat("example-model")
                    .do_stream(options)
                    .await
                    .unwrap()
            } else {
                test.provider
                    .completion("example-completion")
                    .do_stream(options)
                    .await
                    .unwrap()
            };
            let parts = collect_checked(result).await;
            let outcomes: Vec<_> = parts
                .iter()
                .filter_map(|part| match part {
                    StreamPart::Error { .. } => Some("error"),
                    StreamPart::Finish { .. } => Some("finish"),
                    StreamPart::ToolCall(_) => Some("tool-call"),
                    _ => None,
                })
                .collect();
            assert_eq!(
                outcomes,
                vec!["error"],
                "{family}/{fixture} truncated at {end}"
            );
        }
    }
}

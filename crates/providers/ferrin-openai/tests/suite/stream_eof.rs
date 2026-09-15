//! Truncated provider streams must end in an error and close open parts.

use ferrin_spec::CallOptions;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::StreamPart;
use ferrin_testing::Fixture;
use http::Method;
use pretty_assertions::assert_eq;

use super::common::TestProvider;
use super::common::collect_checked;
use super::common::fixture_bytes;

#[tokio::test]
async fn every_preterminal_fixture_boundary_reports_truncation() {
    for (family, fixture) in [
        ("chat", "text-basic-stream"),
        ("chat", "tool-call-stream"),
        ("completion", "text-basic-stream"),
        ("responses", "text-basic-stream"),
        ("responses", "tool-call-stream"),
        ("responses", "reasoning-stream"),
    ] {
        let raw =
            String::from_utf8(fixture_bytes(family, &format!("{fixture}.chunks.txt"))).unwrap();
        let events: Vec<_> = raw.lines().filter(|line| !line.is_empty()).collect();
        let terminal = events
            .iter()
            .position(|line| {
                if family != "responses" {
                    let value: serde_json::Value =
                        serde_json::from_str(line.strip_prefix("data: ").unwrap()).unwrap();
                    value["choices"][0]["finish_reason"].is_string()
                } else {
                    line.contains("response.completed")
                }
            })
            .unwrap();
        for end in 0..=terminal {
            let test = TestProvider::start().await;
            let path = if family == "chat" {
                "/v1/chat/completions"
            } else if family == "completion" {
                "/v1/completions"
            } else {
                "/v1/responses"
            };
            test.mount_fixture(
                Method::POST,
                path,
                Fixture::sse(events[..end].iter().copied()),
            );
            let options = CallOptions::new(vec![PromptMessage::user_text("Hello")]);
            let stream = if family == "chat" {
                test.provider
                    .chat("gpt-4.1")
                    .do_stream(options)
                    .await
                    .unwrap()
            } else if family == "completion" {
                test.provider
                    .completion("gpt-3.5-turbo-instruct")
                    .do_stream(options)
                    .await
                    .unwrap()
            } else {
                test.provider
                    .responses("gpt-4.1")
                    .do_stream(options)
                    .await
                    .unwrap()
            };
            let parts = collect_checked(stream).await;
            let terminal_parts: Vec<_> = parts
                .iter()
                .filter_map(|part| match part {
                    StreamPart::Error { .. } => Some("error"),
                    StreamPart::Finish { .. } => Some("finish"),
                    _ => None,
                })
                .collect();
            assert_eq!(
                terminal_parts,
                vec!["error"],
                "{family}/{fixture} truncated at {end}"
            );
            if family == "chat"
                || !events[..end]
                    .iter()
                    .any(|line| line.contains("response.output_item.done"))
            {
                assert!(
                    !parts
                        .iter()
                        .any(|part| matches!(part, StreamPart::ToolCall(_))),
                    "unfinished tool call at {family}/{fixture}:{end}"
                );
            }
        }
    }
}

//! Usage parity with the pinned reference adapters.

use ferrin_spec::CallOptions;
use ferrin_spec::FileData;
use ferrin_spec::ImageModel;
use ferrin_spec::LanguageModel;
use ferrin_spec::PromptMessage;
use ferrin_spec::StreamPart;
use ferrin_spec::Usage;
use ferrin_spec::image_model::ImageFile;
use ferrin_spec::image_model::ImageOptions;
use ferrin_testing::Fixture;
use http::Method;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::TestProvider;
use super::common::collect_checked;

#[tokio::test]
async fn completion_usage_preserves_partial_totals_breakdowns_and_raw_in_both_modes() {
    for raw in [
        json!({}),
        json!({"prompt_tokens": 7, "custom_counter": 2}),
        json!({"prompt_tokens": 7, "completion_tokens": 3, "total_tokens": 10}),
    ] {
        let test = TestProvider::start().await;
        let response = json!({"id": "completion", "model": "instruct", "choices": [{"text": "ok", "finish_reason": "stop"}], "usage": raw});
        test.mount_fixture(Method::POST, "/v1/completions", Fixture::json(&response));
        let result = test
            .provider
            .completion("instruct")
            .do_generate(CallOptions::new(vec![PromptMessage::user_text("hello")]))
            .await
            .unwrap();
        let mut expected = Usage::default();
        expected.input.total = raw.get("prompt_tokens").and_then(serde_json::Value::as_u64);
        expected.input.no_cache = Some(expected.input.total.unwrap_or(0));
        expected.output.total = raw
            .get("completion_tokens")
            .and_then(serde_json::Value::as_u64);
        expected.output.text = Some(expected.output.total.unwrap_or(0));
        expected.raw = raw.as_object().cloned();
        assert_eq!(result.usage, expected);
        let streamed = TestProvider::start().await;
        streamed.mount_fixture(
            Method::POST,
            "/v1/completions",
            Fixture::sse_json([&response]),
        );
        let parts = collect_checked(
            streamed
                .provider
                .completion("instruct")
                .do_stream(CallOptions::new(vec![PromptMessage::user_text("hello")]))
                .await
                .unwrap(),
        )
        .await;
        let finish = parts
            .into_iter()
            .find_map(|part| {
                if let StreamPart::Finish { usage, .. } = part {
                    Some(usage)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(finish, expected);
    }
}

#[tokio::test]
async fn chat_missing_usage_stays_unknown_but_present_usage_defaults_missing_breakdowns() {
    for raw in [
        None,
        Some(json!({"prompt_tokens": 7, "completion_tokens": 3})),
    ] {
        let test = TestProvider::start().await;
        let response = json!({"id": "chat", "model": "gpt-4o", "choices": [{"message": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}], "usage": raw});
        test.mount_fixture(
            Method::POST,
            "/v1/chat/completions",
            Fixture::json(&response),
        );
        let result = test
            .provider
            .chat("gpt-4o")
            .do_generate(CallOptions::new(vec![PromptMessage::user_text("hello")]))
            .await
            .unwrap();
        let expected: Usage = if raw.is_some() {
            serde_json::from_value(json!({"input": {"total": 7, "no_cache": 7, "cache_read": 0}, "output": {"total": 3, "text": 3, "reasoning": 0}, "raw": raw})).unwrap()
        } else {
            Usage::default()
        };
        assert_eq!(result.usage, expected);
        let streamed = TestProvider::start().await;
        let chunk = json!({"id": "chat", "model": "gpt-4o", "choices": [{"delta": {"role": "assistant", "content": "ok"}, "finish_reason": "stop"}], "usage": raw});
        streamed.mount_fixture(
            Method::POST,
            "/v1/chat/completions",
            Fixture::sse_json([&chunk]),
        );
        let parts = collect_checked(
            streamed
                .provider
                .chat("gpt-4o")
                .do_stream(CallOptions::new(vec![PromptMessage::user_text("hello")]))
                .await
                .unwrap(),
        )
        .await;
        let finish = parts
            .into_iter()
            .find_map(|part| {
                if let StreamPart::Finish { usage, .. } = part {
                    Some(usage)
                } else {
                    None
                }
            })
            .unwrap();
        assert_eq!(finish, expected);
    }
}

#[tokio::test]
async fn image_token_details_keep_remainders_for_generation_and_edits() {
    for path in ["/v1/images/generations", "/v1/images/edits"] {
        let test = TestProvider::start().await;
        let response = json!({"data": [{"b64_json": "YQ=="}, {"b64_json": "Yg=="}, {"b64_json": "Yw=="}], "usage": {"input_tokens_details": {"image_tokens": 8, "text_tokens": 2}}});
        test.mount_fixture(Method::POST, path, Fixture::json(&response));
        let mut options = ImageOptions::new("three images");
        options.n = 3;
        if path.ends_with("edits") {
            options.files.push(ImageFile {
                data: FileData::Bytes {
                    data: bytes::Bytes::from_static(b"image"),
                },
                media_type: Some("image/png".into()),
                provider_options: None,
            });
        }
        let result = test
            .provider
            .image("gpt-image-1")
            .do_generate(options)
            .await
            .unwrap();
        assert_eq!(
            result.provider_metadata.unwrap()["openai"]["images"],
            json!([
                {"imageTokens": 2, "textTokens": 0},
                {"imageTokens": 2, "textTokens": 0},
                {"imageTokens": 4, "textTokens": 2},
            ])
        );
    }
}

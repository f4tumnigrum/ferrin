use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use ferrin_core::LanguageModelMiddleware;
use ferrin_core::generate_text;
use ferrin_core::stream_text;
use ferrin_core::wrap_language_model;
use ferrin_policy::PolicyError;
use ferrin_policy::capability_middleware;
use ferrin_policy::policy_client;
use ferrin_spec::Content;
use ferrin_spec::DynLanguageModel;
use ferrin_spec::FinishReason;
use ferrin_spec::GenerateResult;
use ferrin_spec::JsonValue;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolCall;
use ferrin_spec::ToolChoice;
use ferrin_spec::Usage;
use ferrin_testing::MockLanguageModel;
use ferrin_tool::Schema;
use ferrin_tool::Tool;
use ferrin_tool::ToolContext;
use ferrin_tool::ToolError;
use ferrin_tool::ToolSet;
use pretty_assertions::assert_eq;
use serde_json::json;

fn tools(calls: &Arc<AtomicUsize>) -> ToolSet {
    let calls = Arc::clone(calls);
    let tool = Tool::function_with_schema(Schema::from_json_schema(json!({ "type": "object" })))
        .execute(move |_input: JsonValue, _ctx: ToolContext| {
            calls.fetch_add(1, Ordering::SeqCst);
            async { Ok::<_, ToolError>(json!({ "executed": true })) }
        })
        .build();
    ToolSet::new().insert("delete_file", tool).unwrap()
}

fn call_model() -> Arc<MockLanguageModel> {
    let call = ToolCall::new("call-1", "delete_file", "{}");
    MockLanguageModel::builder()
        .generate_repeat(GenerateResult::new(
            vec![Content::ToolCall(call.clone())],
            FinishReason::tool_calls(),
        ))
        .stream_repeat(vec![
            StreamPart::stream_start(),
            StreamPart::ToolCall(call),
            StreamPart::finish(FinishReason::tool_calls(), Usage::default()),
        ])
        .build_shared()
}

fn wrap(
    model: Arc<MockLanguageModel>,
    response: Result<JsonValue, PolicyError>,
) -> Arc<dyn DynLanguageModel> {
    wrap_language_model(
        model,
        [Arc::new(capability_middleware(
            policy_client(move |_, _| response.clone()),
            "p",
        )) as Arc<dyn LanguageModelMiddleware>],
    )
}

#[tokio::test]
async fn filtered_tools_never_execute_in_generate_or_stream() {
    for response in [
        Ok(json!([])),
        Ok(json!(null)),
        Err(PolicyError::Engine {
            message: "unavailable".into(),
        }),
    ] {
        let calls = Arc::new(AtomicUsize::new(0));
        let model = wrap(call_model(), response);
        let generated = generate_text(Arc::clone(&model))
            .prompt("delete it")
            .tools(tools(&calls))
            .await
            .unwrap();
        let streamed = stream_text(model)
            .prompt("delete it")
            .tools(tools(&calls))
            .await
            .unwrap()
            .consume()
            .await
            .unwrap();
        assert_eq!(
            (
                calls.load(Ordering::SeqCst),
                generated.steps.len(),
                streamed.steps.len()
            ),
            (0, 1, 1)
        );
    }
}

#[tokio::test]
async fn allowed_tools_still_execute_in_generate_and_stream() {
    let calls = Arc::new(AtomicUsize::new(0));
    let model = wrap(call_model(), Ok(json!(["delete_file"])));
    generate_text(Arc::clone(&model))
        .prompt("delete it")
        .tools(tools(&calls))
        .await
        .unwrap();
    stream_text(model)
        .prompt("delete it")
        .tools(tools(&calls))
        .await
        .unwrap()
        .consume()
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn removed_tool_choices_allow_text_in_generate_and_stream() {
    for choice in [ToolChoice::Required, ToolChoice::tool("delete_file")] {
        let calls = Arc::new(AtomicUsize::new(0));
        let model = MockLanguageModel::builder()
            .generate_repeat(GenerateResult::new(
                vec![Content::text("ok")],
                FinishReason::stop(),
            ))
            .stream_repeat(vec![
                StreamPart::stream_start(),
                StreamPart::TextStart {
                    id: "text".into(),
                    provider_metadata: None,
                },
                StreamPart::text_delta("text", "ok"),
                StreamPart::TextEnd {
                    id: "text".into(),
                    provider_metadata: None,
                },
                StreamPart::finish(FinishReason::stop(), Usage::default()),
            ])
            .build_shared();
        let model = wrap(model, Ok(json!([])));
        let generated = generate_text(Arc::clone(&model))
            .prompt("hi")
            .tools(tools(&calls))
            .tool_choice(choice.clone())
            .await
            .unwrap();
        let streamed = stream_text(model)
            .prompt("hi")
            .tools(tools(&calls))
            .tool_choice(choice)
            .await
            .unwrap()
            .consume()
            .await
            .unwrap();
        assert_eq!(
            (
                generated.text(),
                streamed.text(),
                calls.load(Ordering::SeqCst)
            ),
            ("ok".into(), "ok".into(), 0)
        );
    }
}

#[tokio::test]
async fn concurrent_calls_keep_independent_restrictions() {
    let barrier = Arc::new(tokio::sync::Barrier::new(2));
    let mock = MockLanguageModel::builder()
        .generate_with(move |_| {
            let barrier = Arc::clone(&barrier);
            async move {
                barrier.wait().await;
                Ok(GenerateResult::new(
                    vec![Content::ToolCall(ToolCall::new("c", "delete_file", "{}"))],
                    FinishReason::tool_calls(),
                ))
            }
        })
        .build_shared();
    let middleware = capability_middleware(
        policy_client(|_, input| {
            Ok(if input == json!("allow") {
                json!(["delete_file"])
            } else {
                json!([])
            })
        }),
        "p",
    )
    .to_input(|options, _| json!(options.headers.get_str("x-policy")));
    let model = wrap_language_model(
        mock,
        [Arc::new(middleware) as Arc<dyn LanguageModelMiddleware>],
    );
    let allowed = Arc::new(AtomicUsize::new(0));
    let denied = Arc::new(AtomicUsize::new(0));
    let (a, b) = tokio::join!(
        generate_text(Arc::clone(&model))
            .prompt("a")
            .tools(tools(&allowed))
            .headers(ferrin_spec::Headers::new().with("x-policy", "allow")),
        generate_text(model).prompt("b").tools(tools(&denied)),
    );
    a.unwrap();
    b.unwrap();
    assert_eq!(
        (
            allowed.load(Ordering::SeqCst),
            denied.load(Ordering::SeqCst)
        ),
        (1, 0)
    );
}

#[tokio::test]
async fn request_retries_use_a_fresh_policy_contract() {
    let evaluations = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&evaluations);
    let middleware = capability_middleware(
        policy_client(move |_, _| {
            Ok(if seen.fetch_add(1, Ordering::SeqCst).is_multiple_of(2) {
                json!([])
            } else {
                json!(["delete_file"])
            })
        }),
        "p",
    );
    let call = ToolCall::new("call-1", "delete_file", "{}");
    let mock = MockLanguageModel::builder()
        .generate_error(ferrin_testing::api_call_error(
            http::StatusCode::SERVICE_UNAVAILABLE,
            "retry",
        ))
        .generate(GenerateResult::new(
            vec![Content::ToolCall(call.clone())],
            FinishReason::tool_calls(),
        ))
        .stream_error(ferrin_testing::api_call_error(
            http::StatusCode::SERVICE_UNAVAILABLE,
            "retry",
        ))
        .stream(vec![
            StreamPart::stream_start(),
            StreamPart::ToolCall(call),
            StreamPart::finish(FinishReason::tool_calls(), Usage::default()),
        ])
        .build_shared();
    let model = wrap_language_model(
        mock,
        [Arc::new(middleware) as Arc<dyn LanguageModelMiddleware>],
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let retry = ferrin_core::retry::RetryPolicy {
        initial_delay: std::time::Duration::ZERO,
        ..Default::default()
    };
    generate_text(Arc::clone(&model))
        .prompt("hi")
        .tools(tools(&calls))
        .retry_policy(retry.clone())
        .await
        .unwrap();
    stream_text(model)
        .prompt("hi")
        .tools(tools(&calls))
        .retry_policy(retry)
        .await
        .unwrap()
        .consume()
        .await
        .unwrap();
    assert_eq!(
        (
            evaluations.load(Ordering::SeqCst),
            calls.load(Ordering::SeqCst)
        ),
        (4, 2)
    );
}

use std::sync::Arc;

use ferrin_core::Agent;
use ferrin_core::AgentCall;
use ferrin_core::Error;
use ferrin_core::ToolLoopAgent;
use ferrin_core::agent::PrepareCallInput;
use ferrin_core::generate_text::ApprovalStatus;
use ferrin_core::generate_text::PrepareStepContext;
use ferrin_core::generate_text::RepairRequest;
use ferrin_core::generate_text::StepOverrides;
use ferrin_core::generate_text::ToolCallRepair;
use ferrin_message::Message;
use ferrin_message::ToolApprovalResponse;
use ferrin_message::UserPart;
use ferrin_spec::BoxFuture;
use ferrin_spec::FinishReason;
use ferrin_spec::PromptMessage;
use ferrin_spec::StreamPart;
use ferrin_spec::ToolCall;
use ferrin_spec::ToolDefinition;
use ferrin_spec::Usage;
use ferrin_tool::Tool;
use ferrin_tool::ToolCaller;
use ferrin_tool::ToolCallerDefinition;
use pretty_assertions::assert_eq;
use secrecy::SecretBox;
use serde_json::json;

use super::common::mock;
use super::common::text_result;
use super::common::tool_call_result;
use super::common::weather_tools;

fn call_parts() -> Vec<StreamPart> {
    vec![
        StreamPart::stream_start(),
        StreamPart::ToolCall(ToolCall::new("call", "get_weather", r#"{"city":"Oslo"}"#)),
        StreamPart::finish(FinishReason::tool_calls(), Usage::default()),
    ]
}

fn secret(value: &[u8]) -> Arc<SecretBox<[u8]>> {
    Arc::new(SecretBox::new(value.into()))
}

#[tokio::test]
async fn preparation_sets_and_clears_approval_and_signing_per_call() {
    for streaming in [false, true] {
        for external_approval in [false, true] {
            let model = mock()
                .generate(tool_call_result(
                    "call",
                    "get_weather",
                    &json!({"city":"Oslo"}),
                ))
                .generate(text_result("done"))
                .stream(call_parts())
                .stream(ferrin_testing::text_parts(["done"], Usage::default()))
                .build_shared();
            let mut guarded = weather_tools();
            let guarded_weather = guarded
                .get("get_weather")
                .unwrap()
                .as_ref()
                .clone()
                .into_builder()
                .needs_approval(ferrin_tool::NeedsApproval::Always)
                .build();
            guarded.replace("get_weather", Arc::new(guarded_weather));
            let agent = ToolLoopAgent::builder(model)
                .tools(guarded)
                .stop_when(ferrin_core::step_count(1))
                .tool_approval(ApprovalStatus::denied())
                .tool_approval_secret(SecretBox::new(b"inherited-secret".as_slice().into()))
                .call_options::<bool>()
                .prepare_call(|mut input: PrepareCallInput<bool>| async move {
                    input.defaults.tool_approval = input
                        .options
                        .then(|| Arc::new(ApprovalStatus::user_approval()) as _);
                    input.defaults.tool_approval_secret =
                        input.options.then(|| secret(b"prepared-secret"));
                    Ok(input.defaults)
                })
                .build();
            let call = AgentCall::prompt("weather").options(external_approval);
            let first = if streaming {
                agent
                    .stream(call.streaming())
                    .await
                    .unwrap()
                    .consume()
                    .await
                    .unwrap()
            } else {
                agent.generate(call).await.unwrap()
            };
            let request = first.last_step().tool_approval_requests().next().unwrap();
            assert_eq!(
                (request.signature.is_some(), request.is_automatic),
                (external_approval, false)
            );
            assert_eq!(first.last_step().tool_results().count(), 0);
            let mut history = vec![Message::user("weather")];
            history.extend(first.response_messages());
            history.push(Message::tool([ToolApprovalResponse::approved(
                request.approval_id.clone(),
            )]));
            let call = AgentCall::messages(history).options(external_approval);
            let resumed = if streaming {
                agent
                    .stream(call.streaming())
                    .await
                    .unwrap()
                    .consume()
                    .await
                    .unwrap()
            } else {
                agent.generate(call).await.unwrap()
            };
            assert_eq!(resumed.text(), "done");
        }
    }
}

struct RepairWeather;

impl ToolCallRepair for RepairWeather {
    fn repair<'a>(
        &'a self,
        request: RepairRequest<'a>,
    ) -> BoxFuture<'a, Result<Option<ToolCall>, ferrin_core::error::BoxError>> {
        let mut repaired = request.tool_call.clone();
        repaired.tool_name = "get_weather".into();
        repaired.input = json!({"city":"initial"}).to_string();
        Box::pin(async move { Ok(Some(repaired)) })
    }
}

#[tokio::test]
async fn preparation_selects_step_repair_refinement_and_tool_callers() {
    for streaming in [false, true] {
        let model = mock()
            .generate(tool_call_result("call", "unknown", &json!({})))
            .stream(vec![
                StreamPart::stream_start(),
                StreamPart::ToolCall(ToolCall::new("call", "unknown", "{}")),
                StreamPart::finish(FinishReason::tool_calls(), Usage::default()),
            ])
            .build_shared();
        let tools = weather_tools()
            .insert(
                "caller",
                Tool::provider_executed("test.caller", Default::default())
                    .caller(ToolCallerDefinition::provider(|options| {
                        let mut options = options.unwrap_or_default();
                        options
                            .entry("test".into())
                            .or_default()
                            .insert("caller".into(), json!("prepared"));
                        options
                    }))
                    .build(),
            )
            .unwrap();
        let agent = ToolLoopAgent::builder(Arc::clone(&model))
            .tools(tools)
            .stop_when(ferrin_core::step_count(1))
            .prepare_step(|_: &PrepareStepContext<'_>| {
                StepOverrides::none().with_instructions("inherited")
            })
            .prepare_call(|mut input: PrepareCallInput<()>| async move {
                input.defaults.prepare_step = Some(Arc::new(|_: &PrepareStepContext<'_>| {
                    StepOverrides::none().with_instructions("prepared")
                }));
                input.defaults.repair_tool_call = Some(Arc::new(RepairWeather));
                input.defaults.refine_tool_inputs.insert(
                    "get_weather",
                    Arc::new(|_| Box::pin(async { Ok(json!({"city":"normalized"})) })),
                );
                input.defaults.tool_callers.insert(
                    "get_weather".into(),
                    vec![ToolCaller::Direct, ToolCaller::Tool("caller".into())],
                );
                Ok(input.defaults)
            })
            .build();
        let call = AgentCall::prompt("weather");
        let result = if streaming {
            agent
                .stream(call.streaming())
                .await
                .unwrap()
                .consume()
                .await
                .unwrap()
        } else {
            agent.generate(call).await.unwrap()
        };
        assert_eq!(
            result.last_step().tool_results().next().unwrap().output,
            json!({"city":"normalized","temperature":21})
        );
        let calls = if streaming {
            model.stream_calls()
        } else {
            model.generate_calls()
        };
        assert_eq!(calls[0].prompt[0], PromptMessage::system("prepared"));
        let ToolDefinition::Function {
            provider_options, ..
        } = &calls[0].tools[1]
        else {
            panic!("expected weather definition")
        };
        assert_eq!(
            provider_options.as_ref().unwrap()["test"]["caller"],
            json!("prepared")
        );
    }
}

struct TestDownloader(&'static [u8]);

impl ferrin_core::prompt::DownloadFn for TestDownloader {
    fn download(
        &self,
        requests: Vec<ferrin_core::prompt::DownloadRequest>,
        _: tokio_util::sync::CancellationToken,
    ) -> BoxFuture<'_, Result<Vec<Option<ferrin_core::prompt::DownloadedFile>>, Error>> {
        let data = bytes::Bytes::from_static(self.0);
        Box::pin(async move {
            Ok(requests
                .into_iter()
                .map(|_| {
                    Some(ferrin_core::prompt::DownloadedFile {
                        data: data.clone(),
                        media_type: Some("image/png".into()),
                    })
                })
                .collect())
        })
    }
}

#[tokio::test]
async fn preparation_selects_the_file_downloader() {
    for streaming in [false, true] {
        let model = mock()
            .generate(text_result("done"))
            .stream(ferrin_testing::text_parts(["done"], Usage::default()))
            .build_shared();
        let agent = ToolLoopAgent::builder(Arc::clone(&model))
            .prepare_call(|mut input: PrepareCallInput<()>| async move {
                input.defaults.download = Some(Arc::new(TestDownloader(b"prepared-image")));
                Ok(input.defaults)
            })
            .build();
        let call = AgentCall::messages([Message::user_parts([UserPart::image_url(
            "https://example.com/image.png".parse().unwrap(),
        )])]);
        if streaming {
            agent
                .stream(call.streaming())
                .await
                .unwrap()
                .consume()
                .await
                .unwrap();
        } else {
            agent.generate(call).await.unwrap();
        }
        let calls = if streaming {
            model.stream_calls()
        } else {
            model.generate_calls()
        };
        let PromptMessage::User { content, .. } = &calls[0].prompt[0] else {
            panic!("expected user")
        };
        let ferrin_spec::language_model::prompt::UserPromptPart::File(file) = &content[0] else {
            panic!("expected file")
        };
        assert_eq!(
            file.data,
            ferrin_spec::FileData::bytes(bytes::Bytes::from_static(b"prepared-image"))
        );
    }
}

#[tokio::test]
async fn preparation_can_clear_inherited_step_repair_refinement_callers_and_download() {
    for streaming in [false, true] {
        for tool_name in ["unknown", "get_weather"] {
            let model = mock()
                .generate(tool_call_result("call", tool_name, &json!({"city":"Oslo"})))
                .stream(vec![
                    StreamPart::stream_start(),
                    StreamPart::ToolCall(ToolCall::new("call", tool_name, r#"{"city":"Oslo"}"#)),
                    StreamPart::finish(FinishReason::tool_calls(), Usage::default()),
                ])
                .supported_urls(ferrin_spec::SupportedUrls::all())
                .build_shared();
            let tool_set = weather_tools()
                .insert(
                    "caller",
                    Tool::provider_executed("test.caller", Default::default())
                        .caller(ToolCallerDefinition::provider(|_| {
                            [(
                                "test".to_owned(),
                                json!({"caller":"inherited"}).as_object().unwrap().clone(),
                            )]
                            .into_iter()
                            .collect()
                        }))
                        .build(),
                )
                .unwrap();
            let agent = ToolLoopAgent::builder(Arc::clone(&model))
                .tools(tool_set)
                .stop_when(ferrin_core::step_count(1))
                .prepare_step(|_: &PrepareStepContext<'_>| {
                    StepOverrides::none().with_instructions("inherited")
                })
                .repair_tool_call(RepairWeather)
                .refine_tool_input("get_weather", |_| {
                    Box::pin(async { Ok(json!({"city":"inherited"})) })
                })
                .tool_callers(
                    [(
                        "get_weather".into(),
                        vec![ToolCaller::Direct, ToolCaller::Tool("caller".into())],
                    )]
                    .into_iter()
                    .collect(),
                )
                .download(Arc::new(TestDownloader(b"inherited-image")))
                .prepare_call(|mut input: PrepareCallInput<()>| async move {
                    input.defaults.prepare_step = None;
                    input.defaults.repair_tool_call = None;
                    input.defaults.refine_tool_inputs = Default::default();
                    input.defaults.tool_callers.clear();
                    input.defaults.download = None;
                    Ok(input.defaults)
                })
                .build();
            let url: url::Url = "https://example.com/image.png".parse().unwrap();
            let call =
                AgentCall::messages([Message::user_parts([UserPart::image_url(url.clone())])]);
            let result = if streaming {
                agent
                    .stream(call.streaming())
                    .await
                    .unwrap()
                    .consume()
                    .await
                    .unwrap()
            } else {
                agent.generate(call).await.unwrap()
            };
            if tool_name == "unknown" {
                assert_eq!(
                    (
                        result.last_step().tool_errors().count(),
                        result.last_step().tool_results().count()
                    ),
                    (1, 0)
                );
            } else {
                assert_eq!(
                    result.last_step().tool_results().next().unwrap().output,
                    json!({"city":"Oslo","temperature":21})
                );
            }
            let calls = if streaming {
                model.stream_calls()
            } else {
                model.generate_calls()
            };
            let PromptMessage::User { content, .. } = &calls[0].prompt[0] else {
                panic!("cleared instructions must not produce a system message")
            };
            let ferrin_spec::language_model::prompt::UserPromptPart::File(file) = &content[0]
            else {
                panic!("expected file")
            };
            assert_eq!(file.data, ferrin_spec::FileData::Url { url });
            let ToolDefinition::Function {
                provider_options, ..
            } = &calls[0].tools[1]
            else {
                panic!("expected weather definition")
            };
            assert_eq!(provider_options, &None);
        }
    }
}

#[cfg(feature = "sandbox")]
#[tokio::test]
async fn preparation_sees_the_invocation_sandbox() {
    let sandbox: Arc<dyn ferrin_tool::Sandbox> =
        Arc::new(ferrin_tool::LocalProcessSandbox::new("."));
    let expected = Arc::clone(&sandbox);
    let agent = ToolLoopAgent::builder(mock().generate(text_result("done")).build_shared())
        .prepare_call(move |input: PrepareCallInput<()>| {
            assert!(Arc::ptr_eq(input.sandbox.as_ref().unwrap(), &expected));
            async { Ok(input.defaults) }
        })
        .build();
    assert_eq!(
        agent
            .generate(AgentCall::prompt("hi").sandbox(sandbox))
            .await
            .unwrap()
            .text(),
        "done"
    );
}

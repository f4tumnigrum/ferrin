use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use ferrin_core::Agent;
use ferrin_core::AgentCall;
use ferrin_core::StepResult;
use ferrin_core::Timeout;
use ferrin_core::ToolLoopAgent;
use ferrin_core::agent::AGENT_USER_AGENT;
use ferrin_core::agent::PrepareCallInput;
use ferrin_core::step_count;
use ferrin_message::Message;
use ferrin_spec::PromptMessage;
use ferrin_spec::Usage;
use ferrin_testing::text_parts;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::mock;
use super::common::text_result;
use super::common::tool_call_result;
use super::common::weather_tools;

fn system_content(message: &PromptMessage) -> Option<&str> {
    match message {
        PromptMessage::System { content, .. } => Some(content),
        _ => None,
    }
}

#[tokio::test]
async fn agent_reports_id_and_tools() {
    let agent = ToolLoopAgent::builder(mock().build_shared())
        .id("assistant")
        .tools(weather_tools())
        .build();
    assert_eq!(agent.id(), Some("assistant"));
    assert_eq!(agent.tools().len(), 1);
    assert!(agent.tools().contains("get_weather"));
}

#[tokio::test]
async fn agent_adds_instructions_and_user_agent_suffix() {
    let model = mock().generate(text_result("hello")).build_shared();
    let agent = ToolLoopAgent::builder(Arc::clone(&model))
        .instructions("You are terse.")
        .build();

    let result = agent.generate(AgentCall::prompt("hi")).await.unwrap();
    assert_eq!(result.text(), "hello");

    let calls = model.generate_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(system_content(&calls[0].prompt[0]), Some("You are terse."));
    assert_eq!(calls[0].prompt.len(), 2);
    assert_eq!(
        calls[0].headers.get_str("user-agent"),
        Some(format!("{AGENT_USER_AGENT} ferrin/{}", env!("CARGO_PKG_VERSION")).as_str())
    );
}

#[tokio::test]
async fn agent_accepts_messages_as_input() {
    let model = mock().generate(text_result("hello")).build_shared();
    let agent = ToolLoopAgent::builder(Arc::clone(&model)).build();

    agent
        .generate(AgentCall::messages([Message::user("hi")]))
        .await
        .unwrap();

    let calls = model.generate_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].prompt.len(), 1);
    assert_eq!(calls[0].prompt[0].role(), "user");
}

#[tokio::test]
async fn agent_stops_after_twenty_steps_by_default() {
    let model = mock()
        .generate_repeat(tool_call_result(
            "call-1",
            "get_weather",
            &json!({ "city": "Rome" }),
        ))
        .build_shared();
    let agent = ToolLoopAgent::builder(Arc::clone(&model))
        .tools(weather_tools())
        .build();

    let result = agent.generate(AgentCall::prompt("weather?")).await.unwrap();
    assert_eq!(result.steps.len(), 20);
    assert_eq!(model.call_count(), 20);
}

#[tokio::test]
async fn agent_stop_condition_overrides_the_default() {
    let model = mock()
        .generate_repeat(tool_call_result(
            "call-1",
            "get_weather",
            &json!({ "city": "Rome" }),
        ))
        .build_shared();
    let agent = ToolLoopAgent::builder(Arc::clone(&model))
        .tools(weather_tools())
        .stop_when(step_count(2))
        .build();

    let result = agent.generate(AgentCall::prompt("weather?")).await.unwrap();
    assert_eq!(result.steps.len(), 2);
}

#[derive(Debug, Clone)]
struct Tenant {
    name: String,
}

#[tokio::test]
async fn prepare_call_rewrites_settings_from_options() {
    let model = mock().generate(text_result("ok")).build_shared();
    let seen_timeout: Arc<Mutex<Option<Timeout>>> = Arc::new(Mutex::new(None));
    let agent = ToolLoopAgent::builder(Arc::clone(&model))
        .instructions("base")
        .timeout(Timeout::none().with_total(Duration::from_secs(30)))
        .call_options::<Tenant>()
        .prepare_call({
            let seen_timeout = Arc::clone(&seen_timeout);
            move |input: PrepareCallInput<Tenant>| {
                let seen_timeout = Arc::clone(&seen_timeout);
                async move {
                    *seen_timeout.lock().unwrap() = Some(input.defaults.timeout.clone());
                    let mut call = input.defaults;
                    assert_eq!(
                        call.instructions.as_ref().map(|i| i.content.as_str()),
                        Some("base")
                    );
                    assert_eq!(call.stop_conditions.len(), 1);
                    call.instructions = Some(format!("Tenant: {}", input.options.name).into());
                    call.settings.temperature = Some(0.2);
                    Ok(call)
                }
            }
        })
        .build();

    agent
        .generate(
            AgentCall::prompt("hi")
                .options(Tenant {
                    name: "acme".to_owned(),
                })
                .timeout(Timeout::none().with_total(Duration::from_secs(1))),
        )
        .await
        .unwrap();

    assert_eq!(
        *seen_timeout.lock().unwrap(),
        Some(Timeout::none().with_total(Duration::from_secs(1)))
    );
    let calls = model.generate_calls();
    assert_eq!(system_content(&calls[0].prompt[0]), Some("Tenant: acme"));
    assert_eq!(calls[0].temperature, Some(0.2));
}

#[tokio::test]
async fn call_hooks_run_after_agent_hooks() {
    let log: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));
    let agent = ToolLoopAgent::builder(mock().generate(text_result("ok")).build_shared())
        .on_step_end({
            let log = Arc::clone(&log);
            move |_step: Arc<StepResult>| {
                log.lock().unwrap().push("agent");
                async {}
            }
        })
        .build();

    agent
        .generate(AgentCall::prompt("hi").on_step_end({
            let log = Arc::clone(&log);
            move |_step: Arc<StepResult>| {
                log.lock().unwrap().push("call");
                async {}
            }
        }))
        .await
        .unwrap();

    assert_eq!(*log.lock().unwrap(), vec!["agent", "call"]);
}

#[tokio::test]
async fn agent_streams_through_stream_text() {
    let model = mock()
        .stream(text_parts(["Hel", "lo"], Usage::totals(3, 2)))
        .build_shared();
    let agent = ToolLoopAgent::builder(Arc::clone(&model))
        .instructions("You are terse.")
        .build();

    let result = agent
        .stream(AgentCall::prompt("hi").streaming())
        .await
        .unwrap()
        .consume()
        .await
        .unwrap();
    assert_eq!(result.text(), "Hello");

    let calls = model.stream_calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(system_content(&calls[0].prompt[0]), Some("You are terse."));
    assert!(
        calls[0]
            .headers
            .get_str("user-agent")
            .is_some_and(|ua| ua.starts_with(AGENT_USER_AGENT))
    );
}

async fn run_generic<A>(agent: &A) -> String
where
    A: Agent<Options = (), Output = ()>,
{
    agent
        .generate(AgentCall::prompt("hi"))
        .await
        .unwrap()
        .text()
}

#[tokio::test]
async fn agent_trait_is_usable_generically() {
    let agent =
        ToolLoopAgent::builder(mock().generate(text_result("generic")).build_shared()).build();
    assert_eq!(run_generic(&agent).await, "generic");
}

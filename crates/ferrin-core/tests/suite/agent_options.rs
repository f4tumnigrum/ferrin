use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use ferrin_core::Agent;
use ferrin_core::AgentCall;
use ferrin_core::Error;
use ferrin_core::Timeout;
use ferrin_core::ToolLoopAgent;
use ferrin_core::agent::PrepareCallInput;
use ferrin_core::telemetry::StartEvent;
use ferrin_core::timeout::TimeoutScope;
use ferrin_schema::Schema;
use ferrin_spec::JsonValue;
use ferrin_spec::Usage;
use ferrin_spec::error::TypeValidationError;
use pretty_assertions::assert_eq;
use serde::Serialize;
use serde_json::json;

use super::common::mock;
use super::common::text_result;

fn options_schema() -> Schema<JsonValue> {
    Schema::with_json_schema_and_validator(json!({"type":"object"}), |value| {
        let Some(name) = value.get("name").and_then(JsonValue::as_str) else {
            return Err(TypeValidationError::new(
                value,
                std::io::Error::other("invalid name"),
            ));
        };
        let name = name.trim().to_lowercase();
        if name.is_empty() {
            return Err(TypeValidationError::new(
                value,
                std::io::Error::other("empty name"),
            ));
        }
        Ok(json!({"name":name,"normalized":true}))
    })
}

#[tokio::test]
async fn schema_normalizes_options_before_preparation_in_both_paths() {
    for streaming in [false, true] {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&seen);
        let agent = ToolLoopAgent::builder(
            mock()
                .generate(text_result("done"))
                .stream(ferrin_testing::text_parts(["done"], Usage::default()))
                .build_shared(),
        )
        .call_options::<JsonValue>()
        .call_options_schema(options_schema())
        .prepare_call(move |input: PrepareCallInput<JsonValue>| {
            recorded.lock().unwrap().push(input.options.clone());
            async move {
                let mut call = input.defaults;
                call.runtime_context = Some(input.options);
                Ok(call)
            }
        })
        .build();
        let call = AgentCall::prompt("hi").options(json!({"name":"  RONIN  ","discarded":true}));
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
        let normalized = json!({"name":"ronin","normalized":true});
        assert_eq!(
            (
                seen.lock().unwrap().clone(),
                result.last_step().runtime_context.clone()
            ),
            (vec![normalized.clone()], Some(normalized))
        );
    }
}

#[tokio::test]
async fn invalid_options_fail_before_preparation_hooks_and_models() {
    for streaming in [false, true] {
        let model = mock().build_shared();
        let hooks = Arc::new(Mutex::new(Vec::new()));
        let recorded_hooks = Arc::clone(&hooks);
        let agent = ToolLoopAgent::builder(Arc::clone(&model))
            .call_options::<JsonValue>()
            .call_options_schema(options_schema())
            .prepare_call(|_: PrepareCallInput<JsonValue>| async {
                panic!("preparation must not run")
            })
            .on_start(move |_: Arc<StartEvent>| {
                recorded_hooks.lock().unwrap().push("started");
                async {}
            })
            .build();
        let call = AgentCall::prompt("hi").options(json!({"secret":"private payload"}));
        let error = if streaming {
            agent.stream(call.streaming()).await.unwrap_err()
        } else {
            agent.generate(call).await.unwrap_err()
        };
        assert!(matches!(&error, Error::InvalidArgument {argument, ..} if argument == "options"));
        assert!(hooks.lock().unwrap().is_empty());
        assert_eq!(
            (error.to_string(), model.call_count()),
            (
                "invalid argument `options`: call options failed schema validation".to_owned(),
                0
            )
        );
    }
}

struct CannotSerialize;

impl Serialize for CannotSerialize {
    fn serialize<S: serde::Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
        Err(serde::ser::Error::custom("private payload"))
    }
}

#[tokio::test]
async fn serialization_failure_is_sanitized_and_validation_remains_optional() {
    let model = mock().build_shared();
    let agent = ToolLoopAgent::builder(Arc::clone(&model))
        .call_options::<CannotSerialize>()
        .call_options_schema(Schema::with_json_schema_and_validator(json!({}), |_| {
            panic!("validator must not run after serialization fails")
        }))
        .build();
    let error = agent
        .generate(AgentCall::prompt("hi").options(CannotSerialize))
        .await
        .unwrap_err();
    assert_eq!(
        (error.to_string(), model.call_count()),
        (
            "invalid argument `options`: cannot serialize call options".to_owned(),
            0
        )
    );

    struct Opaque(std::sync::atomic::AtomicBool);
    let agent = ToolLoopAgent::builder(mock().generate(text_result("done")).build_shared())
        .call_options::<Opaque>()
        .prepare_call(|input: PrepareCallInput<Opaque>| async move {
            assert!(input.options.0.load(std::sync::atomic::Ordering::Relaxed));
            Ok(input.defaults)
        })
        .build();
    assert_eq!(
        agent
            .generate(AgentCall::prompt("hi").options(Opaque(true.into())))
            .await
            .unwrap()
            .text(),
        "done"
    );
}

#[tokio::test(start_paused = true)]
async fn explicit_call_timeout_wins_after_preparation_in_both_paths() {
    for streaming in [false, true] {
        for explicit in [None, Some(Duration::from_secs(1))] {
            let model = mock()
                .generate_with(|_| std::future::pending())
                .stream_with(|_| std::future::pending())
                .build_shared();
            let agent = ToolLoopAgent::builder(model)
                .timeout(Duration::from_secs(30))
                .prepare_call(|mut input: PrepareCallInput<()>| async move {
                    assert_eq!(
                        input.defaults.timeout,
                        Timeout::from(Duration::from_secs(30))
                    );
                    input.defaults.timeout = Timeout::from(Duration::from_secs(3));
                    Ok(input.defaults)
                })
                .build();
            let mut call = AgentCall::prompt("hi");
            if let Some(timeout) = explicit {
                call = call.timeout(timeout);
            }
            let started = tokio::time::Instant::now();
            let error = if streaming {
                agent.stream(call.streaming()).await.unwrap_err()
            } else {
                agent.generate(call).await.unwrap_err()
            };
            assert!(matches!(
                error,
                Error::Timeout {
                    scope: TimeoutScope::Total,
                    ..
                }
            ));
            assert_eq!(
                started.elapsed(),
                explicit.unwrap_or(Duration::from_secs(3))
            );
        }
    }
}

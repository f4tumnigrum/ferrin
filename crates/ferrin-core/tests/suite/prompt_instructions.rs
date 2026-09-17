use std::sync::Arc;
use std::sync::Mutex;

use ferrin_core::Instructions;
use ferrin_core::generate_text;
use ferrin_core::stream_text;
use ferrin_core::telemetry::StartEvent;
use ferrin_core::telemetry::TelemetryOptions;
use ferrin_message::SystemMessage;
use ferrin_spec::PromptMessage;
use ferrin_spec::Usage;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::mock;
use super::common::text_result;

fn instructions() -> Vec<SystemMessage> {
    vec![
        SystemMessage {
            content: "first".into(),
            provider_options: Some(
                serde_json::from_value(json!({"test":{"cache":"first"}})).unwrap(),
            ),
        },
        SystemMessage {
            content: "second".into(),
            provider_options: Some(
                serde_json::from_value(json!({"test":{"cache":"second"}})).unwrap(),
            ),
        },
    ]
}

#[tokio::test]
async fn instruction_arrays_keep_order_metadata_and_empty_semantics_in_both_loops() {
    for streaming in [false, true] {
        for count in 0..=2 {
            let values = instructions()[..count].to_vec();
            let model = mock()
                .generate(text_result("done"))
                .stream(ferrin_testing::text_parts(["done"], Usage::default()))
                .build_shared();
            let captured = Arc::new(Mutex::new(None));
            let recorded = Arc::clone(&captured);
            let on_start = move |event: Arc<StartEvent>| {
                *recorded.lock().unwrap() = event.inputs.as_ref().unwrap().system.clone();
                async {}
            };
            let telemetry = TelemetryOptions {
                record_inputs: true,
                ..TelemetryOptions::enabled()
            };
            if streaming {
                stream_text(Arc::clone(&model))
                    .system(values.clone())
                    .prompt("hi")
                    .telemetry(telemetry)
                    .on_start(on_start)
                    .await
                    .unwrap()
                    .consume()
                    .await
                    .unwrap();
            } else {
                generate_text(Arc::clone(&model))
                    .system(values.clone())
                    .prompt("hi")
                    .telemetry(telemetry)
                    .on_start(on_start)
                    .await
                    .unwrap();
            }
            let calls = if streaming {
                model.stream_calls()
            } else {
                model.generate_calls()
            };
            let expected: Vec<_> = values
                .iter()
                .map(|message| PromptMessage::System {
                    content: message.content.clone(),
                    provider_options: message.provider_options.clone(),
                })
                .chain([PromptMessage::user_text("hi")])
                .collect();
            assert_eq!(
                (&calls[0].prompt, captured.lock().unwrap().clone()),
                (&expected, Some(Instructions::from(values)))
            );
        }
    }
}

#[test]
fn single_message_conversions_and_provider_options_remain_usable() {
    let message = instructions().remove(0);
    let options = message.provider_options.clone().unwrap();
    assert_eq!(
        Instructions::new("first")
            .with_provider_options(options.clone())
            .into_messages(),
        vec![message.clone()]
    );
    assert_eq!(
        Instructions::from(message.clone()).as_messages(),
        &[message]
    );
    let rewritten = Instructions::messages(instructions())
        .with_provider_options(options.clone())
        .into_messages();
    assert_eq!(
        rewritten,
        vec![
            SystemMessage {
                content: "first".into(),
                provider_options: Some(options.clone())
            },
            SystemMessage {
                content: "second".into(),
                provider_options: Some(options)
            },
        ]
    );
}

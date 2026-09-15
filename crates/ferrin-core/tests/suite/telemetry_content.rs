use std::sync::Arc;
use std::sync::Mutex;

use ferrin_core::StepResult;
use ferrin_core::generate_text;
use ferrin_core::generate_text::Include;
use ferrin_core::stream_text;
use ferrin_core::telemetry::EndEvent;
use ferrin_core::telemetry::ModelCallEndEvent;
use ferrin_core::telemetry::StepEndEvent;
use ferrin_core::telemetry::Telemetry;
use ferrin_core::telemetry::TelemetryOptions;
use ferrin_spec::Usage;
use ferrin_spec::Warning;
use pretty_assertions::assert_eq;
use serde_json::json;

use super::common::mock;
use super::common::text_result;

#[derive(Default)]
struct Recorder {
    model_ends: Mutex<Vec<ModelCallEndEvent>>,
    steps: Mutex<Vec<StepResult>>,
    ends: Mutex<Vec<EndEvent>>,
}
impl Telemetry for Recorder {
    fn on_language_model_call_end(&self, event: &ModelCallEndEvent) {
        self.model_ends.lock().unwrap().push(event.clone());
    }
    fn on_step_end(&self, event: &StepEndEvent) {
        self.steps.lock().unwrap().push((*event.step).clone());
    }
    fn on_end(&self, event: &EndEvent) {
        self.ends.lock().unwrap().push(event.clone());
    }
}

#[tokio::test]
async fn telemetry_flags_filter_nested_steps_without_changing_application_hooks() {
    let logged = Arc::new(Mutex::new(Vec::new()));
    let _subscriber = tracing::subscriber::set_default(WarningSubscriber(Arc::clone(&logged)));
    for streaming in [false, true] {
        for record_inputs in [false, true] {
            for record_outputs in [false, true] {
                let recorder = Arc::new(Recorder::default());
                let options = TelemetryOptions {
                    record_inputs,
                    record_outputs,
                    ..TelemetryOptions::enabled().with_integration(recorder.clone())
                };
                let mut response = text_result("SECRET_OUTPUT");
                response.request.body = Some(json!({"prompt":"SECRET_INPUT"}));
                response.response.body = Some(json!({"text":"SECRET_OUTPUT"}));
                let warnings = vec![
                    Warning::other("SECRET_WARNING"),
                    Warning::unsupported_with_details("feature", "SECRET_WARNING"),
                    Warning::compatibility("feature", Some("SECRET_WARNING".into())),
                    Warning::deprecated("setting", "SECRET_WARNING"),
                ];
                response.warnings = warnings.clone();
                let mut parts = ferrin_testing::text_parts(["SECRET_OUTPUT"], Usage::default());
                parts[0] = ferrin_spec::StreamPart::StreamStart { warnings };
                let model = mock().generate(response).stream(parts).build_shared();
                let hooks = Arc::new(Mutex::new(Vec::new()));
                let step_hook = Arc::clone(&hooks);
                let on_step = move |step: Arc<StepResult>| {
                    step_hook.lock().unwrap().push((*step).clone());
                    async {}
                };
                let end_hook = Arc::clone(&hooks);
                let on_end = move |event: Arc<EndEvent>| {
                    end_hook.lock().unwrap().extend(event.steps.iter().cloned());
                    async {}
                };
                let result = if streaming {
                    stream_text(model)
                        .prompt("SECRET_INPUT")
                        .include(Include::all())
                        .telemetry(options)
                        .on_step_end(on_step)
                        .on_end(on_end)
                        .await
                        .unwrap()
                        .consume()
                        .await
                        .unwrap()
                } else {
                    generate_text(model)
                        .prompt("SECRET_INPUT")
                        .include(Include::all())
                        .telemetry(options)
                        .on_step_end(on_step)
                        .on_end(on_end)
                        .await
                        .unwrap()
                };
                assert_eq!(result.text(), "SECRET_OUTPUT");
                assert!(format!("{:?}", result.last_step().warnings).contains("SECRET_WARNING"));
                assert_eq!(
                    *hooks.lock().unwrap(),
                    vec![result.last_step().clone(), result.last_step().clone()]
                );
                let step_events = recorder.steps.lock().unwrap();
                let ends = recorder.ends.lock().unwrap();
                assert_eq!(step_events.len(), 1);
                assert_eq!(ends.len(), 1);
                for step in step_events.iter().chain(ends[0].steps.iter()) {
                    assert_eq!(
                        format!("{:?}", step.warnings).contains("SECRET_WARNING"),
                        record_inputs && record_outputs
                    );
                    assert_eq!(
                        (step.content.is_empty(), step.response.messages.is_empty()),
                        (!record_outputs, !record_outputs)
                    );
                    assert_eq!(step.request.messages.is_some(), record_inputs);
                    if !record_inputs {
                        assert!(step.request.body.is_none());
                    }
                    if !record_outputs {
                        assert!(step.response.body.is_none());
                        assert!(step.provider_metadata.is_none());
                    }
                    assert_eq!(
                        (&step.usage, &step.finish_reason),
                        (&result.last_step().usage, &result.last_step().finish_reason)
                    );
                }
                let model_ends = recorder.model_ends.lock().unwrap();
                assert_eq!(model_ends[0].content.is_some(), record_outputs);
                assert_eq!(
                    format!("{:?}", model_ends[0].warnings).contains("SECRET_WARNING"),
                    record_inputs && record_outputs
                );
                if !record_outputs {
                    assert!(model_ends[0].response.body.is_none());
                }
            }
        }
    }
    let logged = logged.lock().unwrap();
    assert_eq!(logged.len(), 32);
    assert!(logged.iter().all(|event| !event.contains("SECRET_WARNING")));
}

struct WarningSubscriber(Arc<Mutex<Vec<String>>>);
impl tracing::Subscriber for WarningSubscriber {
    fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
        metadata.target() == ferrin_core::telemetry::WARNINGS_TARGET
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Fields(String);
        impl tracing::field::Visit for Fields {
            fn record_debug(&mut self, _: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                self.0.push_str(&format!("{value:?}"));
            }
        }
        let mut fields = Fields(String::new());
        event.record(&mut fields);
        self.0.lock().unwrap().push(fields.0);
    }
}

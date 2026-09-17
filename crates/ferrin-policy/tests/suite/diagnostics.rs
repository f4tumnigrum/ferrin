use ferrin_policy::policy_client;
use serde_json::json;
use std::sync::Arc;

#[derive(Clone)]
struct LogCapture(Arc<std::sync::Mutex<String>>);
impl tracing::Subscriber for LogCapture {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        tracing::span::Id::from_u64(1)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, event: &tracing::Event<'_>) {
        struct Visitor<'a>(&'a mut String);
        impl tracing::field::Visit for Visitor<'_> {
            fn record_debug(&mut self, _: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                use std::fmt::Write;
                write!(self.0, "{value:?}").unwrap();
            }
        }
        event.record(&mut Visitor(&mut self.0.lock().unwrap()));
    }
    fn enter(&self, _: &tracing::span::Id) {}
    fn exit(&self, _: &tracing::span::Id) {}
}

#[tokio::test]
async fn policy_reasons_and_errors_are_not_logged() {
    use ferrin_core::generate_text::ApprovalContext;
    use ferrin_core::generate_text::ApprovalPolicy;
    use ferrin_core::generate_text::ParsedToolCall;
    let logs = Arc::new(std::sync::Mutex::new(String::new()));
    let capture = LogCapture(Arc::clone(&logs));
    let _guard = tracing::subscriber::set_default(capture);
    let policy = ferrin_policy::policy_approval(
        policy_client(|_, _| {
            Ok(json!({ "decision": "deny", "reason": "review-sensitive-prompt" }))
        }),
        "p",
    );
    let call = ParsedToolCall::new("call-1", "delete_file", json!({}));
    let _decision = policy
        .resolve(
            &call,
            ApprovalContext {
                runtime_context: None,
                messages: &[],
                tools_context: None,
            },
        )
        .await;
    let shadow = ferrin_policy::shadow(
        ferrin_core::generate_text::ApprovalStatus::denied().with_reason("review-sensitive-prompt"),
    );
    shadow
        .resolve(
            &call,
            ApprovalContext {
                runtime_context: None,
                messages: &[],
                tools_context: None,
            },
        )
        .await;
    let failed = ferrin_policy::policy_approval(
        policy_client(|_, _| {
            Err(ferrin_policy::PolicyError::Engine {
                message: "review-sensitive-prompt".into(),
            })
        }),
        "p",
    );
    let denied = failed
        .resolve(
            &call,
            ApprovalContext {
                runtime_context: None,
                messages: &[],
                tools_context: None,
            },
        )
        .await;
    assert!(!format!("{denied:?}").contains("review-sensitive-prompt"));
    assert!(
        !logs.lock().unwrap().contains("review-sensitive-prompt"),
        "policy reason was recorded in tracing"
    );
}

#[test]
fn http_debug_redacts_credentials_and_url_payloads() {
    let url = url::Url::parse("https://user:review-password@policy.example/private-secret?token=review-token#fragment-secret").unwrap();
    let builder = ferrin_policy::HttpPolicyClient::builder(url)
        .header("authorization", "Bearer header-secret");
    let builder_debug = format!("{builder:?}");
    let client_debug = format!("{:?}", builder.build().unwrap());
    for output in [builder_debug, client_debug] {
        assert!(output.contains("https://policy.example"));
        for secret in [
            "review-password",
            "review-token",
            "private-secret",
            "fragment-secret",
            "header-secret",
        ] {
            assert!(!output.contains(secret), "unredacted diagnostic: {output}");
        }
    }
}

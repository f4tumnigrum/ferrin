use ferrin_core::middleware::builtin::default_instructions;
use ferrin_spec::language_model::prompt::PromptMessage;
use ferrin_testing::MockLanguageModel;
use pretty_assertions::assert_eq;

use super::common::options;
use super::common::text_result;
use super::common::wrapped;

#[tokio::test]
async fn prepends_system_instructions_only_when_absent() {
    let mock = MockLanguageModel::builder()
        .generate_repeat(text_result("ok"))
        .build_shared();
    let model = wrapped(mock.clone(), default_instructions("Be terse."));
    model.do_generate(options()).await.unwrap();

    let mut with_system = options();
    with_system
        .prompt
        .insert(0, PromptMessage::system("Existing."));
    model.do_generate(with_system).await.unwrap();

    let calls = mock.generate_calls();
    assert_eq!(calls[0].prompt.len(), 2);
    assert!(matches!(
        &calls[0].prompt[0],
        PromptMessage::System { content, .. } if content == "Be terse."
    ));
    assert_eq!(calls[1].prompt.len(), 2);
    assert!(matches!(
        &calls[1].prompt[0],
        PromptMessage::System { content, .. } if content == "Existing."
    ));
}

#[tokio::test]
async fn multiple_defaults_keep_order_and_metadata_and_empty_defaults_are_noops() {
    for count in 0..=2 {
        let defaults: Vec<_> = (0..count)
            .map(|index| ferrin_message::SystemMessage {
                content: format!("default {index}"),
                provider_options: Some(
                    serde_json::from_value(serde_json::json!({"test":{"index":index}})).unwrap(),
                ),
            })
            .collect();
        let mock = MockLanguageModel::builder()
            .generate_repeat(text_result("ok"))
            .build_shared();
        let model = wrapped(mock.clone(), default_instructions(defaults.clone()));
        let original = options();
        model.do_generate(original.clone()).await.unwrap();
        let mut existing = original.clone();
        existing.prompt.insert(0, PromptMessage::system("existing"));
        model.do_generate(existing.clone()).await.unwrap();
        let mut expected = original;
        expected.prompt.splice(
            0..0,
            defaults.into_iter().map(|message| PromptMessage::System {
                content: message.content,
                provider_options: message.provider_options,
            }),
        );
        assert_eq!(
            mock.generate_calls()
                .iter()
                .map(ferrin_spec::CallOptions::to_recordable)
                .collect::<Vec<_>>(),
            vec![expected.to_recordable(), existing.to_recordable()]
        );
    }
}

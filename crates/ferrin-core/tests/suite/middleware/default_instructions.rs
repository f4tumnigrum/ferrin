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

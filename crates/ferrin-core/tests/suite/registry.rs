use std::sync::Arc;

use ferrin_core::Error;
use ferrin_core::create_provider_registry;
use ferrin_core::custom_provider;
use ferrin_core::generate_text;
use ferrin_core::registry::ProviderRegistry;
use ferrin_core::registry::set_default_registry;
use ferrin_spec::ProviderRef;
use ferrin_spec::error::ProviderError;
use pretty_assertions::assert_eq;

use super::common::text_model;

fn registry() -> ProviderRegistry {
    let provider = custom_provider("mock")
        .language_model("m1", text_model("from m1"))
        .language_model("m2", text_model("from m2"))
        .build();
    create_provider_registry([("mock", Arc::new(provider) as ProviderRef)])
}

#[tokio::test]
async fn resolves_provider_and_model_ids() {
    let registry = registry();
    assert_eq!(registry.provider_ids().collect::<Vec<_>>(), vec!["mock"]);
    let model = registry.language_model("mock:m2").unwrap();
    let result = generate_text(model).prompt("hi").await.unwrap();
    assert_eq!(result.text(), "from m2");
    assert_eq!(result.last_step().model.provider.as_str(), "mock");
    assert_eq!(result.last_step().model.model_id.as_str(), "mock-model");
}

#[test]
fn unknown_providers_and_models_are_reported() {
    let registry = registry();
    match registry.language_model("other:m1").unwrap_err() {
        Error::NoSuchProvider(details) => {
            assert_eq!(details.provider_id.as_str(), "other");
            assert_eq!(details.model_id, "other:m1");
            assert_eq!(
                details
                    .available_providers
                    .iter()
                    .map(ferrin_spec::ProviderId::as_str)
                    .collect::<Vec<_>>(),
                vec!["mock"]
            );
        }
        other => panic!("unexpected error {other:?}"),
    }
    match registry.language_model("mock:missing").unwrap_err() {
        Error::Provider(error) => assert!(matches!(*error, ProviderError::NoSuchModel(_))),
        other => panic!("unexpected error {other:?}"),
    }
    assert!(registry.language_model("no-separator").is_err());
}

#[tokio::test]
async fn custom_separator() {
    let provider = custom_provider("mock")
        .language_model("m1", text_model("x"))
        .build();
    let registry = ProviderRegistry::builder()
        .provider("mock", Arc::new(provider) as ProviderRef)
        .separator(" > ")
        .build();
    assert!(registry.language_model("mock > m1").is_ok());
    assert!(registry.language_model("mock:m1").is_err());
}

#[tokio::test]
async fn string_model_ids_need_the_default_registry() {
    let error = generate_text("mock:m1").prompt("hi").await.unwrap_err();
    assert!(
        matches!(error, Error::NoDefaultRegistry { .. }),
        "{error:?}"
    );

    set_default_registry(Arc::new(registry())).unwrap();
    let result = generate_text("mock:m1").prompt("hi").await.unwrap();
    assert_eq!(result.text(), "from m1");
    assert!(set_default_registry(Arc::new(registry())).is_err());
}

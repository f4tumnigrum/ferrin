use std::sync::Arc;

use ferrin_core::Error;
use ferrin_core::create_provider_registry;
use ferrin_core::custom_provider;
use ferrin_core::generate_text;
use ferrin_core::registry::ProviderRegistry;
use ferrin_core::registry::set_default_registry;
use ferrin_spec::Provider;
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
            assert_eq!(details.model_id, "other");
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

#[test]
fn registry_middleware_wraps_embedding_and_image_models() {
    use ferrin_core::EmbeddingModelMiddleware;
    use ferrin_core::ImageModelMiddleware;
    use ferrin_spec::DynEmbeddingModel;
    use ferrin_spec::DynImageModel;
    use ferrin_spec::ModelId;

    use crate::suite::modalities::common::EmbedMock;
    use crate::suite::modalities::common::ImageMock;

    struct RenameEmbedding;

    impl EmbeddingModelMiddleware for RenameEmbedding {
        fn override_model_id(&self, _model: &dyn DynEmbeddingModel) -> Option<ModelId> {
            Some(ModelId::new("embedding-wrapped"))
        }
    }

    struct RenameImage;

    impl ImageModelMiddleware for RenameImage {
        fn override_model_id(&self, _model: &dyn DynImageModel) -> Option<ModelId> {
            Some(ModelId::new("image-wrapped"))
        }
    }

    let provider = custom_provider("mock")
        .embedding_model("e1", EmbedMock::new())
        .image_model("i1", ImageMock::new())
        .build();
    let registry = ProviderRegistry::builder()
        .provider("mock", Arc::new(provider) as ProviderRef)
        .embedding_model_middleware(Arc::new(RenameEmbedding))
        .image_model_middleware(Arc::new(RenameImage))
        .build();

    let embedding = registry.embedding_model("mock:e1").unwrap();
    assert_eq!(
        embedding.model().unwrap().model_id().as_str(),
        "embedding-wrapped"
    );
    let image = registry.image_model("mock:i1").unwrap();
    assert_eq!(image.model().unwrap().model_id().as_str(), "image-wrapped");
    assert!(registry.embedding_model("mock:missing").is_err());
    assert!(registry.image_model("other:i1").is_err());
}

#[test]
fn malformed_model_id_is_not_an_unknown_provider() {
    let error = registry().language_model("no-separator").unwrap_err();
    assert!(matches!(
        error.as_provider(),
        Some(ProviderError::NoSuchModel(_))
    ));
}

#[tokio::test]
async fn replacing_provider_affects_new_lookups_only() {
    let mut registry = registry();
    let old = registry.language_model("mock:m1").unwrap();
    registry.register_provider(
        "mock",
        Arc::new(
            custom_provider("replacement")
                .language_model("m1", text_model("replacement answer"))
                .build(),
        ),
    );
    let new = registry.language_model("mock:m1").unwrap();
    assert_eq!(
        (
            generate_text(old).prompt("hi").await.unwrap().text(),
            generate_text(new).prompt("hi").await.unwrap().text()
        ),
        ("from m1".to_owned(), "replacement answer".to_owned()),
    );
}

struct Services(ferrin_spec::ProviderId);

impl ferrin_spec::Files for Services {
    fn provider(&self) -> &ferrin_spec::ProviderId {
        &self.0
    }

    async fn upload_file(
        &self,
        _options: ferrin_spec::files::UploadFileOptions,
    ) -> Result<ferrin_spec::files::UploadFileResult, ProviderError> {
        Err(ProviderError::unsupported("fixture file upload"))
    }
}

impl ferrin_spec::Skills for Services {
    fn provider(&self) -> &ferrin_spec::ProviderId {
        &self.0
    }

    async fn upload_skill(
        &self,
        _options: ferrin_spec::skills::UploadSkillOptions,
    ) -> Result<ferrin_spec::skills::UploadSkillResult, ProviderError> {
        Err(ProviderError::unsupported("fixture skill upload"))
    }
}

#[test]
fn custom_provider_services_override_fallback_and_resolve_through_registry() {
    let fallback_files: ferrin_spec::FilesRef = Services("fallback-files".into()).into();
    let fallback_skills: ferrin_spec::SkillsRef = Services("fallback-skills".into()).into();
    let fallback: ProviderRef = Arc::new(
        custom_provider("fallback")
            .files(fallback_files.clone())
            .skills(fallback_skills.clone())
            .build(),
    );
    let inherited = custom_provider("inherited")
        .fallback(fallback.clone())
        .build();
    assert!(Arc::ptr_eq(
        inherited.files().unwrap().inner(),
        fallback_files.inner()
    ));
    assert!(Arc::ptr_eq(
        inherited.skills().unwrap().inner(),
        fallback_skills.inner()
    ));

    let files: ferrin_spec::FilesRef = Services("files".into()).into();
    let skills: ferrin_spec::SkillsRef = Services("skills".into()).into();
    let provider = custom_provider("explicit")
        .files(files.clone())
        .skills(skills.clone())
        .fallback(fallback)
        .build();
    let registry = ProviderRegistry::builder()
        .provider("cloud:region", Arc::new(provider))
        .build();
    assert!(Arc::ptr_eq(
        registry.files("cloud:region").unwrap().inner(),
        files.inner()
    ));
    assert!(Arc::ptr_eq(
        registry.skills("cloud:region").unwrap().inner(),
        skills.inner()
    ));
}

#[test]
fn registry_services_distinguish_missing_providers_and_unsupported_services() {
    let registry = registry();
    for error in [
        registry.files("other").err().unwrap(),
        registry.skills("other").err().unwrap(),
    ] {
        let Error::NoSuchProvider(details) = error else {
            panic!("expected unknown provider")
        };
        assert_eq!(
            (
                details.provider_id,
                details.model_id,
                details.available_providers
            ),
            ("other".into(), "other".into(), vec!["mock".into()])
        );
    }
    for error in [
        registry.files("mock").err().unwrap(),
        registry.skills("mock").err().unwrap(),
    ] {
        assert!(
            matches!(
                error.as_provider(),
                Some(ProviderError::UnsupportedFunctionality(_))
            ),
            "{error:?}"
        );
    }
}

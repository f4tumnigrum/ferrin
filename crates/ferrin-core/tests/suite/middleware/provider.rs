use std::sync::Arc;

use ferrin_core::EmbeddingModelMiddleware;
use ferrin_core::ImageModelMiddleware;
use ferrin_core::LanguageModelMiddleware;
use ferrin_core::ProviderMiddleware;
use ferrin_core::custom_provider;
use ferrin_core::wrap_provider;
use ferrin_spec::DynEmbeddingModel;
use ferrin_spec::DynImageModel;
use ferrin_spec::DynLanguageModel;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ProviderRef;
use pretty_assertions::assert_eq;

use crate::suite::common::text_model;
use crate::suite::modalities::common::EmbedMock;
use crate::suite::modalities::common::ImageMock;

struct RenameLanguage;

impl LanguageModelMiddleware for RenameLanguage {
    fn override_model_id(&self, _model: &dyn DynLanguageModel) -> Option<ModelId> {
        Some(ModelId::new("language-wrapped"))
    }
}

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

fn provider() -> ProviderRef {
    Arc::new(
        custom_provider("mock")
            .language_model("m1", text_model("hi"))
            .language_model("by-id", "other:model")
            .embedding_model("e1", EmbedMock::new())
            .image_model("i1", ImageMock::new())
            .build(),
    )
}

fn all_kinds() -> ProviderMiddleware {
    ProviderMiddleware::new()
        .language_model(Arc::new(RenameLanguage))
        .embedding_model(Arc::new(RenameEmbedding))
        .image_model(Arc::new(RenameImage))
}

#[test]
fn wraps_language_embedding_and_image_models() {
    let wrapped = wrap_provider(provider(), all_kinds());
    assert_eq!(wrapped.provider_id().as_str(), "mock");

    let language = wrapped.language_model("m1").unwrap();
    assert_eq!(
        language.model().unwrap().model_id().as_str(),
        "language-wrapped"
    );
    let embedding = wrapped.embedding_model("e1").unwrap();
    assert_eq!(
        embedding.model().unwrap().model_id().as_str(),
        "embedding-wrapped"
    );
    let image = wrapped.image_model("i1").unwrap();
    assert_eq!(image.model().unwrap().model_id().as_str(), "image-wrapped");
}

#[test]
fn passes_unknown_models_and_services_through() {
    let wrapped = wrap_provider(provider(), all_kinds());
    let error = wrapped.language_model("missing").unwrap_err();
    assert_eq!(error.model_id, "missing");
    assert_eq!(error.provider, Some(ProviderId::new("mock")));
    assert!(wrapped.transcription_model("t1").is_err());
    assert!(wrapped.speech_model("s1").is_err());
    assert!(wrapped.files().is_none());
    assert!(wrapped.realtime().is_none());
    assert!(wrapped.batch().is_none());
}

#[test]
fn empty_middleware_returns_the_provider() {
    let provider = provider();
    let same = wrap_provider(Arc::clone(&provider), ProviderMiddleware::new());
    assert!(Arc::ptr_eq(&provider, &same));
    assert!(ProviderMiddleware::default().is_empty());
    assert!(!all_kinds().is_empty());
}

#[test]
fn unresolved_references_cannot_be_wrapped() {
    let wrapped = wrap_provider(provider(), all_kinds());
    let error = wrapped.language_model("by-id").unwrap_err();
    assert!(error.to_string().contains("unresolved"), "{error}");

    // Without language middleware the reference passes through untouched.
    let only_images = wrap_provider(
        provider(),
        ProviderMiddleware::new().image_model(Arc::new(RenameImage)),
    );
    let reference = only_images.language_model("by-id").unwrap();
    assert_eq!(reference.unresolved_id(), Some("other:model"));
}

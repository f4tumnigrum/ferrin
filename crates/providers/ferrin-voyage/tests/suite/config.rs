//! Provider factory, settings, credentials and option overrides.

use std::sync::Arc;

use ferrin_spec::Headers;
use ferrin_spec::Provider;
use ferrin_spec::ProviderId;
use ferrin_spec::RerankingModel;
use ferrin_spec::error::ModelKind;
use ferrin_spec::error::NoSuchModelError;
use ferrin_spec::error::ProviderError;
use ferrin_voyage::VoyageConfig;
use ferrin_voyage::VoyageProvider;
use ferrin_voyage::VoyageSettings;
use ferrin_voyage::create_voyage;
use pretty_assertions::assert_eq;
use secrecy::SecretString;
use serde_json::json;
use url::Url;

use crate::common::provider_options;
use crate::common::text_options;

#[test]
fn factory_is_lazy_and_unsupported_kinds_return_lookup_errors() {
    let provider = create_voyage(VoyageSettings::default()).unwrap();
    assert_eq!(
        provider.config().base_url.as_str(),
        "https://api.voyageai.com/v1"
    );
    let identity = ProviderId::new("voyage");
    let errors = vec![
        (
            provider.language_model("unknown").unwrap_err(),
            ModelKind::Language,
        ),
        (
            provider.embedding_model("unknown").unwrap_err(),
            ModelKind::Embedding,
        ),
        (
            provider.image_model("unknown").unwrap_err(),
            ModelKind::Image,
        ),
    ];
    for (error, kind) in errors {
        assert_eq!(
            error,
            NoSuchModelError::unsupported_kind(&identity, "unknown", kind)
        );
    }
    let model = provider.reranking("future-rerank-model");
    assert_eq!(
        (model.provider().as_str(), model.model_id().as_str()),
        ("voyage.reranking", "future-rerank-model")
    );
}

#[test]
fn custom_name_merges_canonical_options_and_preserves_identity() {
    let provider = create_voyage(VoyageSettings {
        name: Some("custom".into()),
        ..VoyageSettings::default()
    })
    .unwrap();
    let mut options = text_options();
    options.provider_options = provider_options(
        "voyage",
        json!({"returnDocuments": true, "truncation": false}),
    );
    options
        .provider_options
        .extend(provider_options("custom", json!({"truncation": true})));
    let model = provider.reranking("rerank-2.5");
    let request = model.prepare_request(&options).unwrap();
    assert_eq!(
        request,
        (
            json!({
                "model": "rerank-2.5",
                "query": "Rust async runtimes",
                "documents": ["A guide to gardening.", "Tokio is an async runtime."],
                "return_documents": true,
                "truncation": true,
            }),
            vec![]
        )
    );
    assert_eq!(
        (provider.provider_id().as_str(), model.provider().as_str()),
        ("custom", "custom.reranking")
    );
}

#[test]
fn explicit_authorization_overrides_the_resolved_key_and_debug_redacts_credentials() {
    let mut config =
        VoyageConfig::new("voyage", Url::parse("https://api.example.com/v1/").unwrap()).unwrap();
    config.api_key = Some(SecretString::from("invalid\nkey"));
    config
        .headers
        .insert("authorization", "Bearer configured-secret")
        .unwrap();
    let headers = config
        .headers(&Headers::new().with("authorization", "Bearer call-secret"))
        .unwrap();
    assert_eq!(headers.get_str("authorization"), Some("Bearer call-secret"));
    let provider = VoyageProvider::from_config(Arc::new(config));
    let rendered = format!("{provider:?}");
    for secret in ["invalid", "configured-secret", "call-secret"] {
        assert!(!rendered.contains(secret));
    }
    let settings = VoyageSettings {
        api_key: Some(SecretString::from("fixture-settings-secret")),
        ..VoyageSettings::default()
    };
    assert!(!format!("{settings:?}").contains("fixture-settings-secret"));
}

#[test]
fn authorization_alone_still_requires_key_resolution() {
    const CHILD: &str = "FERRIN_VOYAGE_KEY_RESOLUTION_TEST";
    if ferrin_provider_util::settings::env_var(CHILD).is_some() {
        let config =
            VoyageConfig::new("voyage", Url::parse("https://api.example.com/v1").unwrap()).unwrap();
        let error = config
            .headers(&Headers::new().with("authorization", "Bearer explicit"))
            .unwrap_err();
        assert!(matches!(error, ProviderError::LoadApiKey(_)));
        return;
    }
    // Child environment isolation avoids process-wide environment mutation.
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "suite::config::authorization_alone_still_requires_key_resolution",
            "--nocapture",
        ])
        .env_remove("VOYAGE_API_KEY")
        .env(CHILD, "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn invalid_keys_and_configuration_are_rejected_without_revealing_values() {
    let provider = create_voyage(VoyageSettings {
        api_key: Some(SecretString::from("invalid\nsecret")),
        ..VoyageSettings::default()
    })
    .unwrap();
    let error = provider.config().headers(&Headers::new()).unwrap_err();
    assert!(matches!(error, ProviderError::InvalidArgument(_)));
    assert!(!error.to_string().contains("secret"));
    for input in [
        "file:///tmp/model",
        "https://user:secret@example.com/v1",
        "https://example.com/v1?secret=true",
        "https://example.com/v1#fragment",
    ] {
        let error = create_voyage(VoyageSettings {
            base_url: Some(Url::parse(input).unwrap()),
            ..VoyageSettings::default()
        })
        .unwrap_err();
        assert!(matches!(error, ProviderError::InvalidArgument(_)));
        assert!(!error.to_string().contains("secret"));
    }
    for name in ["", "   ", "invalid.name"] {
        let error = create_voyage(VoyageSettings {
            name: Some(name.into()),
            ..VoyageSettings::default()
        })
        .unwrap_err();
        assert!(matches!(error, ProviderError::InvalidArgument(_)));
    }
}

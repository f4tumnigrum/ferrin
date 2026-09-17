//! Azure OpenAI factories reusing the OpenAI wire implementations.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! reimplemented in Rust; see `NOTICE`.

use std::fmt;
use std::sync::Arc;

use ferrin_openai::OpenAiProvider;
use ferrin_openai::config::OpenAiConfig;
use ferrin_provider_util::SharedTransport;
use ferrin_provider_util::settings::SettingConfig;
use ferrin_provider_util::settings::load_setting;
use ferrin_spec::EmbeddingModelRef;
use ferrin_spec::Headers;
use ferrin_spec::ImageModelRef;
use ferrin_spec::LanguageModelRef;
use ferrin_spec::Provider;
use ferrin_spec::ProviderId;
use ferrin_spec::SpeechModelRef;
use ferrin_spec::TranscriptionModelRef;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::NoSuchModelError;
use ferrin_spec::error::ProviderError;
use secrecy::SecretString;
use url::Url;

use crate::AzureSettings;
use crate::AzureUrlMode;
use crate::TokenProvider;
use crate::transport::AzureTransport;

struct Config {
    base_url: Url,
    api_version: Option<String>,
    mode: AzureUrlMode,
    api_key: Option<SecretString>,
    token_provider: Option<Arc<dyn TokenProvider>>,
    headers: Headers,
    transport: SharedTransport,
    foundry: bool,
}

/// An Azure OpenAI provider with explicitly configured authentication.
#[derive(Clone)]
pub struct AzureProvider {
    config: Arc<Config>,
    id: ProviderId,
}

impl fmt::Debug for AzureProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AzureProvider")
            .field("base_url", &self.config.base_url)
            .field("url_mode", &self.config.mode)
            .finish_non_exhaustive()
    }
}

/// Creates an Azure OpenAI provider without resolving credentials or making requests.
///
/// # Errors
///
/// Returns an error for conflicting authentication, missing resource, an invalid
/// endpoint or a transport initialization failure.
pub fn create_azure(settings: AzureSettings) -> Result<AzureProvider, ProviderError> {
    if settings.api_key.is_some() && settings.token_provider.is_some() {
        return Err(invalid(
            "authentication",
            "choose either api_key or token_provider",
        ));
    }
    let mut base_url = match settings.base_url {
        Some(url) => url,
        None => {
            let resource = load_setting(SettingConfig {
                value: settings.resource_name,
                environment_variable: "AZURE_RESOURCE_NAME",
                setting_name: "resource_name",
                description: "Azure OpenAI resource",
            })?;
            if resource.is_empty()
                || !resource
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
            {
                return Err(invalid("resource_name", "invalid azure resource name"));
            }
            Url::parse(&format!("https://{resource}.openai.azure.com/openai"))
                .map_err(|_| invalid("resource_name", "invalid azure resource name"))?
        }
    };
    if !matches!(base_url.scheme(), "https" | "http")
        || base_url.host_str().is_none()
        || !base_url.username().is_empty()
        || base_url.password().is_some()
        || base_url.query().is_some()
        || base_url.fragment().is_some()
    {
        return Err(invalid(
            "base_url",
            "azure base URL must be HTTP(S) without credentials, query or fragment",
        ));
    }
    let host = base_url.host_str().unwrap_or_default();
    let azure_host = [
        ".openai.azure.com",
        ".services.ai.azure.com",
        ".cognitiveservices.azure.com",
    ]
    .iter()
    .any(|suffix| host.ends_with(suffix));
    let foundry =
        host.ends_with(".services.ai.azure.com") && base_url.path().starts_with("/api/projects/");
    let path = base_url.path().trim_end_matches('/').to_owned();
    let versioned = path.to_ascii_lowercase().ends_with("/openai/v1");
    base_url.set_path(&path);
    let api_version = match settings.url_mode {
        AzureUrlMode::Deployment => Some(settings.api_version.unwrap_or_else(|| "v1".to_owned())),
        AzureUrlMode::V1 => {
            if azure_host && !versioned {
                base_url.set_path(&format!("{path}/v1"));
            }
            (azure_host && !versioned && !foundry)
                .then(|| settings.api_version.unwrap_or_else(|| "v1".to_owned()))
        }
    };
    if api_version
        .as_ref()
        .is_some_and(|version| version.trim().is_empty())
    {
        return Err(invalid(
            "api_version",
            "azure API version must not be empty",
        ));
    }
    let transport = match settings.transport {
        Some(transport) => transport,
        None => ferrin_provider_util::default_transport().map_err(ProviderError::other)?,
    };
    Ok(AzureProvider {
        config: Arc::new(Config {
            base_url,
            api_version,
            mode: settings.url_mode,
            api_key: settings.api_key,
            token_provider: settings.token_provider,
            headers: settings.headers,
            transport,
            foundry,
        }),
        id: ProviderId::new("azure"),
    })
}

fn invalid(argument: &str, message: &str) -> ProviderError {
    InvalidArgumentError::new(argument, message).into()
}

impl AzureProvider {
    fn inner(&self, deployment: &str) -> OpenAiProvider {
        let mut base_url = self.config.base_url.clone();
        if self.config.mode == AzureUrlMode::Deployment {
            let deployment = percent_encoding::utf8_percent_encode(
                deployment,
                percent_encoding::NON_ALPHANUMERIC,
            );
            base_url.set_path(&format!(
                "{}/deployments/{deployment}",
                base_url.path().trim_end_matches('/')
            ));
        }
        let transport = Arc::new(AzureTransport {
            inner: Arc::clone(&self.config.transport),
            api_key: self.config.api_key.clone(),
            token_provider: self.config.token_provider.clone(),
            base_url: base_url.clone(),
            api_version: self.config.api_version.clone(),
            valid_deployment: !deployment.is_empty()
                && deployment != "."
                && deployment != ".."
                && deployment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')),
        });
        let mut config = OpenAiConfig::with_external_authentication("azure", base_url, transport);
        config.headers = self.config.headers.clone();
        config.file_id_prefixes = vec!["assistant-".to_owned()];
        config.explicit_message_item_type = self.config.foundry;
        OpenAiProvider::from_config(Arc::new(config))
    }

    /// Creates a Responses model for an Azure deployment.
    #[must_use]
    pub fn responses(&self, deployment: &str) -> ferrin_openai::OpenAiResponsesLanguageModel {
        self.inner(deployment).responses(deployment)
    }
    /// Creates a Chat Completions model for an Azure deployment.
    #[must_use]
    pub fn chat(&self, deployment: &str) -> ferrin_openai::OpenAiChatLanguageModel {
        self.inner(deployment).chat(deployment)
    }
    /// Creates a Completions model for an Azure deployment.
    #[must_use]
    pub fn completion(&self, deployment: &str) -> ferrin_openai::OpenAiCompletionLanguageModel {
        self.inner(deployment).completion(deployment)
    }
    /// Creates an embedding model for an Azure deployment.
    #[must_use]
    pub fn embedding(&self, deployment: &str) -> ferrin_openai::OpenAiEmbeddingModel {
        self.inner(deployment).embedding(deployment)
    }
    /// Creates an image model for an Azure deployment.
    #[must_use]
    pub fn image(&self, deployment: &str) -> ferrin_openai::OpenAiImageModel {
        self.inner(deployment).image(deployment)
    }
    /// Creates a speech model for an Azure deployment.
    #[must_use]
    pub fn speech(&self, deployment: &str) -> ferrin_openai::OpenAiSpeechModel {
        self.inner(deployment).speech(deployment)
    }
    /// Creates a non-streaming transcription model for an Azure deployment.
    #[must_use]
    pub fn transcription(&self, deployment: &str) -> crate::AzureTranscriptionModel {
        crate::AzureTranscriptionModel(self.inner(deployment).transcription(deployment))
    }
    /// Returns OpenAI tool factories used by Azure deployments that support them.
    #[must_use]
    pub fn tools(&self) -> ferrin_openai::OpenAiTools {
        ferrin_openai::OpenAiTools::new()
    }
}

impl Provider for AzureProvider {
    fn provider_id(&self) -> &ProviderId {
        &self.id
    }
    fn language_model(&self, model_id: &str) -> Result<LanguageModelRef, NoSuchModelError> {
        Ok(self.responses(model_id).into())
    }
    fn embedding_model(&self, model_id: &str) -> Result<EmbeddingModelRef, NoSuchModelError> {
        Ok(self.embedding(model_id).into())
    }
    fn image_model(&self, model_id: &str) -> Result<ImageModelRef, NoSuchModelError> {
        Ok(self.image(model_id).into())
    }
    fn speech_model(&self, model_id: &str) -> Result<SpeechModelRef, NoSuchModelError> {
        Ok(self.speech(model_id).into())
    }
    fn transcription_model(
        &self,
        model_id: &str,
    ) -> Result<TranscriptionModelRef, NoSuchModelError> {
        Ok(self.transcription(model_id).into())
    }
}

//! Skills API (`<name>.skills`).

use std::collections::BTreeSet;

use bytes::Bytes;
use ferrin_provider_util::MultipartForm;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::get;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_form;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderId;
use ferrin_spec::ProviderReference;
use ferrin_spec::error::ProviderError;
use ferrin_spec::skills::SkillFileData;
use ferrin_spec::skills::Skills;
use ferrin_spec::skills::UploadSkillOptions;
use ferrin_spec::skills::UploadSkillResult;
use serde::Deserialize;

use crate::config::SharedConfig;
use crate::error::failed_response_handler;
use crate::output::anthropic_metadata;
use crate::path::encode_path_segment;

/// Beta flag of the Skills API.
pub const SKILLS_BETA: &str = "skills-2025-10-02";

#[derive(Debug, Deserialize)]
struct SkillResponse {
    id: String,
    #[serde(default)]
    display_title: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    latest_version: Option<String>,
    #[serde(default)]
    source: Option<String>,
    #[serde(default)]
    created_at: Option<String>,
    #[serde(default)]
    updated_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SkillVersionResponse {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

/// Skills service backed by `POST /skills`.
#[derive(Debug, Clone)]
pub struct AnthropicSkills {
    config: SharedConfig,
    provider: ProviderId,
}

impl AnthropicSkills {
    /// Creates the service.
    #[must_use]
    pub fn new(config: SharedConfig) -> Self {
        Self {
            provider: config.provider_id("skills"),
            config,
        }
    }
}

impl Skills for AnthropicSkills {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    #[tracing::instrument(skip_all)]
    async fn upload_skill(
        &self,
        options: UploadSkillOptions,
    ) -> Result<UploadSkillResult, ProviderError> {
        let mut form = MultipartForm::new();
        if let Some(title) = &options.display_title {
            form = form.field("display_title", title.clone());
        }
        for file in &options.files {
            let data = match &file.data {
                SkillFileData::Data { data } => data.clone(),
                SkillFileData::Text { text } => Bytes::from(text.clone()),
                #[allow(unreachable_patterns, reason = "SkillFileData is non-exhaustive")]
                _ => return Err(ProviderError::unsupported("skill file data type")),
            };
            form = form.file("files[]", Some(file.path.clone()), None, data);
        }
        let betas = BTreeSet::from([SKILLS_BETA.to_owned()]);
        let headers = self.config.headers(&options.headers, &betas)?;
        let handlers = ResponseHandlers::new(
            json_response_handler::<SkillResponse>(),
            failed_response_handler(),
        );
        let response = post_form(
            self.config.transport.as_ref(),
            self.config.url("/skills"),
            headers.clone(),
            form,
            &handlers,
            options.cancellation.clone(),
        )
        .await?
        .value;
        let mut name = response.name;
        let mut description = response.description;
        if let Some(version) = &response.latest_version {
            let handlers = ResponseHandlers::new(
                json_response_handler::<SkillVersionResponse>(),
                failed_response_handler(),
            );
            let version_response = get(
                self.config.transport.as_ref(),
                self.config.url(&format!(
                    "/skills/{}/versions/{}",
                    encode_path_segment(&response.id),
                    encode_path_segment(version)
                )),
                headers,
                &handlers,
                options.cancellation,
            )
            .await?
            .value;
            if version_response.name.is_some() {
                name = version_response.name;
            }
            if version_response.description.is_some() {
                description = version_response.description;
            }
        }
        let mut reference = ProviderReference::new();
        reference.insert(self.config.name.clone(), response.id);
        let mut meta = JsonObject::new();
        if let Some(source) = response.source {
            meta.insert("source".to_owned(), JsonValue::from(source));
        }
        if let Some(created_at) = response.created_at {
            meta.insert("createdAt".to_owned(), JsonValue::from(created_at));
        }
        if let Some(updated_at) = response.updated_at {
            meta.insert("updatedAt".to_owned(), JsonValue::from(updated_at));
        }
        Ok(UploadSkillResult {
            provider_reference: reference,
            display_title: response.display_title,
            name,
            description,
            latest_version: response.latest_version,
            provider_metadata: Some(anthropic_metadata(meta)),
            warnings: Vec::new(),
        })
    }
}

//! Skills service (`<name>.skills`).

use bytes::Bytes;
use ferrin_provider_util::MultipartForm;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_form;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderId;
use ferrin_spec::ProviderReference;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::skills::SkillFileData;
use ferrin_spec::skills::Skills;
use ferrin_spec::skills::UploadSkillOptions;
use ferrin_spec::skills::UploadSkillResult;
use serde::Deserialize;

use crate::config::SharedConfig;
use crate::embedding::compact;
use crate::embedding::provider_metadata;
use crate::error::failed_response_handler;

#[derive(Debug, Deserialize)]
struct SkillResponse {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    default_version: Option<String>,
    #[serde(default)]
    latest_version: Option<String>,
    #[serde(default)]
    created_at: Option<i64>,
    #[serde(default)]
    updated_at: Option<i64>,
}

/// Skills service backed by `POST /skills`.
#[derive(Debug, Clone)]
pub struct OpenAiSkills {
    config: SharedConfig,
    provider: ProviderId,
}

impl OpenAiSkills {
    /// Creates the service.
    #[must_use]
    pub fn new(config: SharedConfig) -> Self {
        Self {
            provider: config.provider_id("skills"),
            config,
        }
    }
}

impl Skills for OpenAiSkills {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    async fn upload_skill(
        &self,
        options: UploadSkillOptions,
    ) -> Result<UploadSkillResult, ProviderError> {
        let mut warnings = Vec::new();
        if options.display_title.is_some() {
            warnings.push(Warning::unsupported_with_details(
                "displayTitle",
                "OpenAI derives the skill name from SKILL.md; displayTitle is ignored",
            ));
        }
        let mut form = MultipartForm::new();
        for file in &options.files {
            let data = match &file.data {
                SkillFileData::Data { data } => data.clone(),
                SkillFileData::Text { text } => Bytes::from(text.clone()),
                #[allow(unreachable_patterns, reason = "SkillFileData is non-exhaustive")]
                _ => return Err(ProviderError::unsupported("skill file data type")),
            };
            form = form.file("files[]", Some(file.path.clone()), None, data);
        }
        let handlers = ResponseHandlers::new(
            json_response_handler::<SkillResponse>(),
            failed_response_handler(),
        );
        let response = post_form(
            self.config.transport.as_ref(),
            self.config.url("/skills"),
            self.config.headers(&options.headers)?,
            form,
            &handlers,
            options.cancellation,
        )
        .await?;
        let value = response.value;
        let mut reference = ProviderReference::new();
        reference.insert(self.config.name.clone(), value.id.clone());
        let mut meta = JsonObject::new();
        meta.insert(
            "defaultVersion".to_owned(),
            value
                .default_version
                .map_or(JsonValue::Null, JsonValue::from),
        );
        meta.insert(
            "createdAt".to_owned(),
            value.created_at.map_or(JsonValue::Null, JsonValue::from),
        );
        meta.insert(
            "updatedAt".to_owned(),
            value.updated_at.map_or(JsonValue::Null, JsonValue::from),
        );
        Ok(UploadSkillResult {
            provider_reference: reference,
            display_title: None,
            name: value.name,
            description: value.description,
            latest_version: value.latest_version,
            provider_metadata: Some(provider_metadata(
                &self.config.provider_options_key,
                compact(meta),
            )),
            warnings,
        })
    }
}

//! Image model (`<name>.image`).

use base64::Engine;
use ferrin_provider_util::MultipartForm;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_form;
use ferrin_provider_util::http::post_json;
use ferrin_spec::FileData;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::Warning;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use ferrin_spec::image_model::GeneratedImage;
use ferrin_spec::image_model::ImageFile;
use ferrin_spec::image_model::ImageModel;
use ferrin_spec::image_model::ImageOptions;
use ferrin_spec::image_model::ImageResult;
use ferrin_spec::image_model::ImageUsage;
use serde::Deserialize;

use crate::config::SharedConfig;
use crate::error::failed_response_handler;
use crate::metadata::compact;
use crate::options_key::passthrough_options;
use crate::options_key::warn_if_deprecated_key;

/// Maximum images per call.
pub const MAX_IMAGES_PER_CALL: usize = 10;

#[derive(Debug, Deserialize)]
struct ImageResponse {
    #[serde(default)]
    data: Vec<ImageData>,
    #[serde(default)]
    usage: Option<ImageResponseUsage>,
}

#[derive(Debug, Deserialize)]
struct ImageData {
    #[serde(default)]
    b64_json: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ImageResponseUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    total_tokens: Option<u64>,
}

/// Image model backed by `POST /images/generations` and, with input files,
/// multipart `POST /images/edits`.
#[derive(Debug, Clone)]
pub struct OpenAiCompatibleImageModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl OpenAiCompatibleImageModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id("image"),
            config,
            model_id: model_id.into(),
        }
    }

    /// Shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    fn image_bytes(
        file: &ImageFile,
        what: &str,
    ) -> Result<(bytes::Bytes, Option<String>), ProviderError> {
        match &file.data {
            FileData::Bytes { data } => Ok((
                data.clone(),
                file.media_type.as_ref().map(|m| m.as_str().to_owned()),
            )),
            _ => Err(UnsupportedFunctionalityError::with_message(
                format!("{what} data type"),
                format!("{what} must be provided as inline bytes for image edits"),
            )
            .into()),
        }
    }

    fn form_value(value: &JsonValue) -> String {
        match value {
            JsonValue::String(text) => text.clone(),
            other => other.to_string(),
        }
    }
}

impl ImageModel for OpenAiCompatibleImageModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn max_images_per_call(&self) -> Option<usize> {
        Some(MAX_IMAGES_PER_CALL)
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_generate(&self, options: ImageOptions) -> Result<ImageResult, ProviderError> {
        let mut warnings = Vec::new();
        if options.aspect_ratio.is_some() {
            warnings.push(Warning::unsupported_with_details(
                "aspectRatio",
                "This model does not support aspect ratio. Use `size` instead.",
            ));
        }
        if options.seed.is_some() {
            warnings.push(Warning::unsupported("seed"));
        }
        warn_if_deprecated_key(&self.config.name, &options.provider_options, &mut warnings);
        let args = passthrough_options(&self.config.name, &options.provider_options, &[]);
        let size = options.size.as_ref().map(ToString::to_string);
        let handlers = ResponseHandlers::new(
            json_response_handler::<ImageResponse>(),
            failed_response_handler(self.config.error_structure.clone()),
        );
        let headers = self.config.headers(&options.headers)?;
        let response = if options.files.is_empty() {
            let mut body = JsonObject::new();
            body.insert("model".to_owned(), JsonValue::from(self.model_id.as_str()));
            body.insert(
                "prompt".to_owned(),
                options
                    .prompt
                    .clone()
                    .map_or(JsonValue::Null, JsonValue::from),
            );
            body.insert("n".to_owned(), JsonValue::from(options.n));
            body.insert(
                "size".to_owned(),
                size.map_or(JsonValue::Null, JsonValue::from),
            );
            body.extend(args);
            let body = compact(body);
            post_json(
                self.config.transport.as_ref(),
                self.config.url("/images/generations"),
                headers,
                &body,
                &handlers,
                options.cancellation,
            )
            .await?
        } else {
            let mut form = MultipartForm::new().field("model", self.model_id.as_str());
            if let Some(prompt) = &options.prompt {
                form = form.field("prompt", prompt);
            }
            let image_field = if options.files.len() == 1 {
                "image"
            } else {
                "image[]"
            };
            for file in &options.files {
                let (data, media_type) = Self::image_bytes(file, "image file")?;
                form = form.file(image_field, Some("image".to_owned()), media_type, data);
            }
            if let Some(mask) = &options.mask {
                let (data, media_type) = Self::image_bytes(mask, "mask")?;
                form = form.file("mask", Some("mask".to_owned()), media_type, data);
            }
            form = form.field("n", options.n.to_string());
            if let Some(size) = &size {
                form = form.field("size", size);
            }
            for (key, value) in &args {
                if !value.is_null() {
                    form = form.field(key, Self::form_value(value));
                }
            }
            post_form(
                self.config.transport.as_ref(),
                self.config.url("/images/edits"),
                headers,
                form,
                &handlers,
                options.cancellation,
            )
            .await?
        };
        let value = response.value;
        let engine = base64::engine::general_purpose::STANDARD;
        let mut images = Vec::with_capacity(value.data.len());
        for item in &value.data {
            let Some(b64) = &item.b64_json else {
                return Err(InvalidResponseDataError::new(
                    "image response item did not contain b64_json",
                    JsonValue::Null,
                )
                .into());
            };
            let data = engine.decode(b64).map_err(|error| {
                ProviderError::InvalidResponseData(Box::new(InvalidResponseDataError::new(
                    format!("image data is not valid base64: {error}"),
                    JsonValue::Null,
                )))
            })?;
            let media_type = ferrin_provider_util::media_type::detect_media_type(&data);
            images.push(GeneratedImage {
                data: data.into(),
                media_type,
            });
        }
        Ok(ImageResult {
            images,
            is_retryable: None,
            warnings,
            provider_metadata: None,
            response: ResponseMetadata {
                id: None,
                timestamp: Some(chrono::Utc::now()),
                model_id: Some(self.model_id.clone()),
                headers: Some(response.response_headers),
                body: response.raw,
            },
            usage: value.usage.map(|usage| ImageUsage {
                input_tokens: usage.input_tokens,
                output_tokens: usage.output_tokens,
                total_tokens: usage.total_tokens,
            }),
        })
    }
}

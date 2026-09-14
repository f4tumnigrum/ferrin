//! Image model (`<name>.image`).

use base64::Engine;
use ferrin_provider_util::MultipartForm;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_form;
use ferrin_provider_util::http::post_json;
use ferrin_provider_util::provider_options::parse_provider_options;
use ferrin_spec::FileData;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::MediaType;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::Warning;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::UnsupportedFunctionalityError;
use ferrin_spec::image_model::GeneratedImage;
use ferrin_spec::image_model::ImageFile;
use ferrin_spec::image_model::ImageModel;
use ferrin_spec::image_model::ImageOptions;
use ferrin_spec::image_model::ImageResult;
use ferrin_spec::image_model::ImageUsage;
use serde::Deserialize;
use serde_json::json;

use crate::config::SharedConfig;
use crate::embedding::compact;
use crate::embedding::provider_metadata;
use crate::error::failed_response_handler;
use crate::stream_util::timestamp_from_seconds;

/// Provider options of the image model.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageProviderOptions {
    /// Quality (`standard`, `hd`, `low`, `medium`, `high`, `auto`).
    #[serde(default)]
    pub quality: Option<String>,
    /// Style (`vivid`, `natural`; DALL·E 3).
    #[serde(default)]
    pub style: Option<String>,
    /// Background (`transparent`, `opaque`, `auto`).
    #[serde(default)]
    pub background: Option<String>,
    /// Moderation level (`low`, `auto`).
    #[serde(default)]
    pub moderation: Option<String>,
    /// Output format (`png`, `jpeg`, `webp`).
    #[serde(default)]
    pub output_format: Option<String>,
    /// Output compression (0 to 100).
    #[serde(default)]
    pub output_compression: Option<u32>,
    /// Input fidelity for edits (`low`, `high`).
    #[serde(default)]
    pub input_fidelity: Option<String>,
    /// Stable end-user identifier.
    #[serde(default)]
    pub user: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ImageResponse {
    #[serde(default)]
    created: Option<f64>,
    #[serde(default)]
    data: Vec<ImageData>,
    #[serde(default)]
    background: Option<String>,
    #[serde(default)]
    output_format: Option<String>,
    #[serde(default)]
    size: Option<String>,
    #[serde(default)]
    quality: Option<String>,
    #[serde(default)]
    usage: Option<ImageResponseUsage>,
}

#[derive(Debug, Deserialize)]
struct ImageData {
    #[serde(default)]
    b64_json: Option<String>,
    #[serde(default)]
    revised_prompt: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct ImageResponseUsage {
    #[serde(default)]
    input_tokens: Option<u64>,
    #[serde(default)]
    output_tokens: Option<u64>,
    #[serde(default)]
    total_tokens: Option<u64>,
    #[serde(default)]
    input_tokens_details: Option<ImageInputTokensDetails>,
}

#[derive(Debug, Default, Deserialize)]
struct ImageInputTokensDetails {
    #[serde(default)]
    image_tokens: Option<u64>,
    #[serde(default)]
    text_tokens: Option<u64>,
}

/// Maximum images per call for a model id.
#[must_use]
pub fn max_images_per_call(model_id: &str) -> usize {
    match model_id {
        "dall-e-3" => 1,
        "dall-e-2" => 10,
        id if id.starts_with("gpt-image-") || id == "chatgpt-image-latest" => 10,
        _ => 1,
    }
}

/// Whether the model returns base64 without an explicit `response_format`.
fn returns_base64_by_default(model_id: &str) -> bool {
    model_id.starts_with("chatgpt-image-") || model_id.starts_with("gpt-image-")
}

/// Image model backed by `POST /images/generations` and `/images/edits`.
#[derive(Debug, Clone)]
pub struct OpenAiImageModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl OpenAiImageModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id("image"),
            config,
            model_id: model_id.into(),
        }
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
}

impl ImageModel for OpenAiImageModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn max_images_per_call(&self) -> Option<usize> {
        Some(max_images_per_call(self.model_id.as_str()))
    }

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
        let openai = parse_provider_options::<ImageProviderOptions>(
            &self.config.provider_options_key,
            &options.provider_options,
        )?
        .unwrap_or_default();
        let size = options.size.as_ref().map(ToString::to_string);
        let handlers = ResponseHandlers::new(
            json_response_handler::<ImageResponse>(),
            failed_response_handler(),
        );
        let headers = self.config.headers(&options.headers)?;
        let response = if options.files.is_empty() {
            let mut body = json!({
                "model": self.model_id.as_str(),
                "prompt": options.prompt,
                "n": options.n,
                "size": size,
                "quality": openai.quality,
                "style": openai.style,
                "background": openai.background,
                "moderation": openai.moderation,
                "output_format": openai.output_format,
                "output_compression": openai.output_compression,
                "user": openai.user,
            });
            if let Some(object) = body.as_object_mut() {
                if !returns_base64_by_default(self.model_id.as_str()) {
                    object.insert("response_format".to_owned(), JsonValue::from("b64_json"));
                }
                let compacted = compact(std::mem::take(object));
                *object = compacted;
            }
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
            for file in &options.files {
                let (data, media_type) = Self::image_bytes(file, "image file")?;
                form = form.file("image[]", Some("image".to_owned()), media_type, data);
            }
            if let Some(mask) = &options.mask {
                let (data, media_type) = Self::image_bytes(mask, "mask")?;
                form = form.file("mask", Some("mask".to_owned()), media_type, data);
            }
            form = form.field("n", options.n.to_string());
            for (name, value) in [
                ("size", size.as_deref()),
                ("quality", openai.quality.as_deref()),
                ("background", openai.background.as_deref()),
                ("output_format", openai.output_format.as_deref()),
                ("input_fidelity", openai.input_fidelity.as_deref()),
                ("user", openai.user.as_deref()),
            ] {
                if let Some(value) = value {
                    form = form.field(name, value);
                }
            }
            if let Some(compression) = openai.output_compression {
                form = form.field("output_compression", compression.to_string());
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
        let mut entries = Vec::with_capacity(value.data.len());
        let count = u64::try_from(value.data.len().max(1)).unwrap_or(1);
        let details = value
            .usage
            .as_ref()
            .and_then(|u| u.input_tokens_details.as_ref());
        for item in &value.data {
            let Some(b64) = &item.b64_json else {
                continue;
            };
            let data = engine.decode(b64).map_err(|error| {
                ProviderError::InvalidResponseData(Box::new(
                    ferrin_spec::error::InvalidResponseDataError::new(
                        format!("image data is not valid base64: {error}"),
                        JsonValue::Null,
                    ),
                ))
            })?;
            let media_type = value
                .output_format
                .as_deref()
                .map(|format| MediaType::new(format!("image/{format}")))
                .or_else(|| ferrin_provider_util::media_type::detect_media_type(&data));
            images.push(GeneratedImage {
                data: data.into(),
                media_type,
            });
            let mut entry = JsonObject::new();
            entry.insert(
                "revisedPrompt".to_owned(),
                item.revised_prompt
                    .clone()
                    .map_or(JsonValue::Null, JsonValue::from),
            );
            entry.insert(
                "created".to_owned(),
                value.created.map_or(JsonValue::Null, JsonValue::from),
            );
            entry.insert(
                "size".to_owned(),
                value.size.clone().map_or(JsonValue::Null, JsonValue::from),
            );
            entry.insert(
                "quality".to_owned(),
                value
                    .quality
                    .clone()
                    .map_or(JsonValue::Null, JsonValue::from),
            );
            entry.insert(
                "background".to_owned(),
                value
                    .background
                    .clone()
                    .map_or(JsonValue::Null, JsonValue::from),
            );
            entry.insert(
                "outputFormat".to_owned(),
                value
                    .output_format
                    .clone()
                    .map_or(JsonValue::Null, JsonValue::from),
            );
            if let Some(details) = details {
                if let Some(tokens) = details.image_tokens {
                    entry.insert("imageTokens".to_owned(), JsonValue::from(tokens / count));
                }
                if let Some(tokens) = details.text_tokens {
                    entry.insert("textTokens".to_owned(), JsonValue::from(tokens / count));
                }
            }
            entries.push(JsonValue::Object(compact(entry)));
        }
        let mut meta = JsonObject::new();
        meta.insert("images".to_owned(), JsonValue::Array(entries));
        Ok(ImageResult {
            images,
            is_retryable: None,
            warnings,
            provider_metadata: Some(provider_metadata(&self.config.provider_options_key, meta)),
            response: ResponseMetadata {
                id: None,
                timestamp: Some(
                    timestamp_from_seconds(value.created).unwrap_or_else(chrono::Utc::now),
                ),
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

//! Gemini image generation (`generateContent` with the `IMAGE` modality).

use ferrin_spec::Content;
use ferrin_spec::FileData;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ProviderMetadata;
use ferrin_spec::ProviderOptions;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::ToolDefinition;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::image_model::GeneratedImage;
use ferrin_spec::image_model::ImageModel;
use ferrin_spec::image_model::ImageOptions;
use ferrin_spec::image_model::ImageResult;
use ferrin_spec::image_model::ImageUsage;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::GenerateResult;
use ferrin_spec::language_model::LanguageModel;
use ferrin_spec::language_model::PromptMessage;
use ferrin_spec::language_model::prompt::FilePart;
use ferrin_spec::language_model::prompt::TextPart;
use ferrin_spec::language_model::prompt::UserPromptPart;
use ferrin_spec::shared::Warning;

use crate::config::CANONICAL_OPTIONS_KEY;
use crate::config::GoogleConfig;
use crate::config::SharedConfig;
use crate::language_model::GoogleLanguageModel;
use crate::prepare_tools::ids;

/// Default maximum images per call.
pub const DEFAULT_MAX_IMAGES_PER_CALL: usize = 10;

/// Image model backed by a Gemini image-capable language model.
#[derive(Debug, Clone)]
pub struct GoogleImageModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
    max_images_per_call: usize,
}

impl GoogleImageModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: ProviderId::new(config.name.clone()),
            config,
            model_id: model_id.into(),
            max_images_per_call: DEFAULT_MAX_IMAGES_PER_CALL,
        }
    }

    /// Overrides the maximum number of images per call.
    #[must_use]
    pub fn with_max_images_per_call(mut self, max: usize) -> Self {
        self.max_images_per_call = max;
        self
    }

    /// Builds the language model call for `options`.
    ///
    /// # Errors
    ///
    /// Returns [`ProviderError::InvalidArgument`] for non-Gemini model ids,
    /// masks and `n > 1`.
    pub fn prepare_call(
        &self,
        options: &ImageOptions,
    ) -> Result<(CallOptions, Vec<Warning>), ProviderError> {
        if !self.model_id.as_str().starts_with("gemini-") {
            return Err(InvalidArgumentError::new(
                "model_id",
                "Google image models other than Gemini are not supported; use a model id that starts with `gemini-`",
            )
            .into());
        }
        if options.mask.is_some() {
            return Err(InvalidArgumentError::new(
                "mask",
                "Gemini image models do not support mask-based image editing",
            )
            .into());
        }
        if options.n > 1 {
            return Err(InvalidArgumentError::new(
                "n",
                "Gemini image models do not support generating a set number of images per call; use n=1 or omit n",
            )
            .into());
        }
        let mut warnings = Vec::new();
        if options.size.is_some() {
            warnings.push(Warning::unsupported_with_details(
                "size",
                "This model does not support the `size` option. Use `aspectRatio` instead.",
            ));
        }
        let mut content = Vec::new();
        if let Some(prompt) = &options.prompt {
            content.push(UserPromptPart::Text(TextPart::new(prompt.clone())));
        }
        for file in &options.files {
            let media_type = match &file.data {
                FileData::Url { .. } => "image/*".to_owned(),
                _ => file
                    .media_type
                    .as_ref()
                    .map_or_else(|| "image/*".to_owned(), |media| media.as_str().to_owned()),
            };
            content.push(UserPromptPart::File(FilePart::new(
                file.data.clone(),
                media_type,
            )));
        }
        let mut google: JsonObject = options
            .provider_options
            .get(self.config.options_key())
            .or_else(|| options.provider_options.get(CANONICAL_OPTIONS_KEY))
            .cloned()
            .unwrap_or_default();
        let google_search = google.remove("googleSearch");
        google.remove("responseModalities");
        let mut image_config = google.remove("imageConfig").and_then(|value| match value {
            JsonValue::Object(object) => Some(object),
            _ => None,
        });
        if let Some(ratio) = &options.aspect_ratio {
            image_config
                .get_or_insert_with(JsonObject::new)
                .insert("aspectRatio".to_owned(), JsonValue::from(ratio.to_string()));
        }
        google.insert(
            "responseModalities".to_owned(),
            JsonValue::Array(vec![JsonValue::from("IMAGE")]),
        );
        if let Some(image_config) = image_config {
            google.insert("imageConfig".to_owned(), JsonValue::Object(image_config));
        }
        let mut provider_options = ProviderOptions::new();
        provider_options.insert(CANONICAL_OPTIONS_KEY.to_owned(), google);
        let mut call = CallOptions::new(vec![PromptMessage::user(content)]);
        call.seed = options.seed;
        call.provider_options = provider_options;
        call.headers = options.headers.clone();
        call.cancellation = options.cancellation.clone();
        if let Some(search) = google_search {
            let args = match search {
                JsonValue::Object(object) => object,
                _ => JsonObject::new(),
            };
            call.tools.push(ToolDefinition::provider(
                ids::GOOGLE_SEARCH,
                "google_search",
                args,
            ));
        }
        Ok((call, warnings))
    }
}

/// Converts the language model result of an image call into an image result:
/// image files become the generated images, the `google` metadata gains an
/// `images` array and the token usage is summed.
#[must_use]
pub fn image_result(
    config: &GoogleConfig,
    model_id: ModelId,
    result: GenerateResult,
    warnings: Vec<Warning>,
) -> ImageResult {
    let mut images = Vec::new();
    for part in &result.content {
        if let Content::File {
            data: FileData::Bytes { data },
            media_type,
            ..
        } = part
            && media_type.as_str().starts_with("image/")
        {
            images.push(GeneratedImage {
                data: data.clone(),
                media_type: Some(media_type.clone()),
            });
        }
    }
    let mut metadata = result
        .provider_metadata
        .as_ref()
        .and_then(|metadata| metadata.get(CANONICAL_OPTIONS_KEY))
        .cloned()
        .unwrap_or_default();
    metadata.insert(
        "images".to_owned(),
        JsonValue::Array(
            images
                .iter()
                .map(|_| JsonValue::Object(JsonObject::new()))
                .collect(),
        ),
    );
    let mut provider_metadata = ProviderMetadata::new();
    if config.options_key() != CANONICAL_OPTIONS_KEY {
        provider_metadata.insert(config.options_key().to_owned(), metadata.clone());
    }
    provider_metadata.insert(CANONICAL_OPTIONS_KEY.to_owned(), metadata);
    let input = result.usage.input.total;
    let output = result.usage.output.total;
    let mut all_warnings = warnings;
    all_warnings.extend(result.warnings);
    ImageResult {
        images,
        is_retryable: (result.finish_reason.unified == FinishReasonKind::ContentFilter)
            .then_some(false),
        warnings: all_warnings,
        provider_metadata: Some(provider_metadata),
        response: ResponseMetadata {
            id: result.response.id,
            timestamp: Some(chrono::Utc::now()),
            model_id: Some(model_id),
            headers: result.response.headers,
            body: result.response.body,
        },
        usage: Some(ImageUsage {
            input_tokens: input,
            output_tokens: output,
            total_tokens: Some(input.unwrap_or_default() + output.unwrap_or_default()),
        }),
    }
}

impl ImageModel for GoogleImageModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn max_images_per_call(&self) -> Option<usize> {
        Some(self.max_images_per_call)
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_generate(&self, options: ImageOptions) -> Result<ImageResult, ProviderError> {
        let (call, warnings) = self.prepare_call(&options)?;
        let language_model = GoogleLanguageModel::new(self.config.clone(), self.model_id.clone());
        let result = language_model.do_generate(call).await?;
        Ok(image_result(
            &self.config,
            self.model_id.clone(),
            result,
            warnings,
        ))
    }
}

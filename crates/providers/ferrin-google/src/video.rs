//! Veo video generation (`predictLongRunning` operations).

use base64::Engine;
use ferrin_provider_util::headers::is_same_origin;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::get;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_spec::FileData;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::MediaType;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::shared::Warning;
use ferrin_spec::video_model::FrameType;
use ferrin_spec::video_model::VideoAspectRatio;
use ferrin_spec::video_model::VideoData;
use ferrin_spec::video_model::VideoFile;
use ferrin_spec::video_model::VideoModel;
use ferrin_spec::video_model::VideoOptions;
use ferrin_spec::video_model::VideoResult;
use ferrin_spec::video_model::VideoStartOptions;
use ferrin_spec::video_model::VideoStartResult;
use ferrin_spec::video_model::VideoStatusOptions;
use ferrin_spec::video_model::VideoStatusResult;
use secrecy::ExposeSecret;
use serde::Deserialize;
use serde_json::json;
use url::Url;

use crate::api_types::RpcStatus;
use crate::config::CANONICAL_OPTIONS_KEY;
use crate::config::SharedConfig;
use crate::error::failed_response_handler;
use crate::output::OutputMapper;

/// Maximum videos per call.
pub const MAX_VIDEOS_PER_CALL: usize = 4;

/// Option keys consumed by this crate; every other key under `google` is
/// passed through into `parameters`.
const CONSUMED_OPTION_KEYS: [&str; 5] = [
    "pollIntervalMs",
    "pollTimeoutMs",
    "personGeneration",
    "negativePrompt",
    "referenceImages",
];

/// A long-running operation.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Operation {
    /// Operation name (`models/veo-x/operations/...`).
    #[serde(default)]
    pub name: Option<String>,
    /// Whether the operation finished.
    #[serde(default)]
    pub done: Option<bool>,
    /// Error of a failed operation.
    #[serde(default)]
    pub error: Option<RpcStatus>,
    /// Response of a finished operation.
    #[serde(default)]
    pub response: Option<JsonValue>,
}

/// Serializes a duration in seconds, as an integer when it has no fractional
/// part (the API declares `durationSeconds` as an integer).
fn seconds_value(seconds: f64) -> JsonValue {
    if seconds.is_finite() && seconds.fract() == 0.0 && seconds >= 0.0 {
        // `seconds` is integral and non-negative, so the conversion is exact
        // for every value the API accepts.
        #[allow(
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss,
            reason = "guarded by the integral and non-negative checks above"
        )]
        return JsonValue::from(seconds as u64);
    }
    json!(seconds)
}

fn image_object(file: &VideoFile, warnings: &mut Vec<Warning>) -> Option<JsonValue> {
    let media_type = file
        .media_type
        .as_ref()
        .map_or("image/png", MediaType::as_str)
        .to_owned();
    match &file.data {
        FileData::Url { url } if url.scheme() == "gs" => {
            Some(json!({"gcsUri": url.as_str(), "mimeType": "image/png"}))
        }
        FileData::Url { .. } | FileData::Reference { .. } => {
            warnings.push(Warning::unsupported_with_details(
                "URL-based image input",
                "Google Generative AI video models require base64-encoded images or GCS URIs. URL will be ignored.",
            ));
            None
        }
        FileData::Bytes { data } => Some(json!({
            "bytesBase64Encoded": base64::engine::general_purpose::STANDARD.encode(data),
            "mimeType": media_type,
        })),
        FileData::Text { text } => Some(json!({
            "bytesBase64Encoded": base64::engine::general_purpose::STANDARD.encode(text.as_bytes()),
            "mimeType": media_type,
        })),
        #[allow(unreachable_patterns, reason = "FileData is non-exhaustive")]
        _ => None,
    }
}

fn reference_image(reference: &JsonValue) -> JsonValue {
    if let Some(bytes) = reference
        .get("bytesBase64Encoded")
        .and_then(JsonValue::as_str)
        .filter(|bytes| !bytes.is_empty())
    {
        return json!({"image": {"bytesBase64Encoded": bytes, "mimeType": "image/png"}, "referenceType": "asset"});
    }
    if let Some(uri) = reference
        .get("gcsUri")
        .and_then(JsonValue::as_str)
        .filter(|uri| !uri.is_empty())
    {
        return json!({"image": {"gcsUri": uri, "mimeType": "image/png"}, "referenceType": "asset"});
    }
    reference.clone()
}

/// A prepared `predictLongRunning` request.
#[derive(Debug, Clone, PartialEq)]
pub struct PreparedVideoRequest {
    /// Request body (`{instances, parameters}`).
    pub body: JsonValue,
    /// Warnings.
    pub warnings: Vec<Warning>,
}

/// Veo video model.
#[derive(Debug, Clone)]
pub struct GoogleVideoModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl GoogleVideoModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: ProviderId::new(config.name.clone()),
            config,
            model_id: model_id.into(),
        }
    }

    /// Builds the request body for `options`.
    #[must_use]
    pub fn prepare_request(&self, options: &VideoOptions) -> PreparedVideoRequest {
        let mut warnings = Vec::new();
        let google: JsonObject = options
            .provider_options
            .get(self.config.options_key())
            .or_else(|| options.provider_options.get(CANONICAL_OPTIONS_KEY))
            .cloned()
            .unwrap_or_default();
        let mut instance = JsonObject::new();
        if let Some(prompt) = &options.prompt {
            instance.insert("prompt".to_owned(), JsonValue::from(prompt.as_str()));
        }
        let first_frame = options
            .frame_images
            .iter()
            .find(|frame| frame.frame_type == FrameType::FirstFrame)
            .map(|frame| &frame.image)
            .or(options.image.as_ref());
        if let Some(image) = first_frame.and_then(|file| image_object(file, &mut warnings)) {
            instance.insert("image".to_owned(), image);
        }
        let last_frame = options
            .frame_images
            .iter()
            .find(|frame| frame.frame_type == FrameType::LastFrame)
            .map(|frame| &frame.image);
        if let Some(image) = last_frame.and_then(|file| image_object(file, &mut warnings)) {
            instance.insert("lastFrame".to_owned(), image);
        }
        if options.frame_images.is_empty() && !options.input_references.is_empty() {
            let references: Vec<JsonValue> = options
                .input_references
                .iter()
                .filter_map(|file| image_object(file, &mut warnings))
                .map(|image| json!({"image": image, "referenceType": "asset"}))
                .collect();
            instance.insert("referenceImages".to_owned(), JsonValue::Array(references));
        } else if let Some(JsonValue::Array(references)) = google.get("referenceImages") {
            instance.insert(
                "referenceImages".to_owned(),
                JsonValue::Array(references.iter().map(reference_image).collect()),
            );
        }
        let mut parameters = JsonObject::new();
        parameters.insert("sampleCount".to_owned(), JsonValue::from(options.n));
        match &options.aspect_ratio {
            Some(VideoAspectRatio::Ratio(ratio)) => {
                parameters.insert("aspectRatio".to_owned(), JsonValue::from(ratio.to_string()));
            }
            Some(VideoAspectRatio::Adaptive) => {
                parameters.insert("aspectRatio".to_owned(), JsonValue::from("adaptive"));
            }
            #[allow(unreachable_patterns, reason = "VideoAspectRatio is non-exhaustive")]
            Some(_) => {}
            None => {}
        }
        if let Some(resolution) = &options.resolution {
            let mapped = match (resolution.width, resolution.height) {
                (1280, 720) => "720p".to_owned(),
                (1920, 1080) => "1080p".to_owned(),
                (3840, 2160) => "4k".to_owned(),
                _ => resolution.to_string(),
            };
            parameters.insert("resolution".to_owned(), JsonValue::from(mapped));
        }
        if let Some(duration) = options.duration.filter(|duration| *duration != 0.0) {
            parameters.insert("durationSeconds".to_owned(), seconds_value(duration));
        }
        if let Some(seed) = options.seed.filter(|seed| *seed != 0) {
            parameters.insert("seed".to_owned(), JsonValue::from(seed));
        }
        if options.fps.is_some() {
            warnings.push(Warning::unsupported("fps"));
        }
        if options.generate_audio.is_some() {
            warnings.push(Warning::unsupported("generateAudio"));
        }
        for key in ["personGeneration", "negativePrompt"] {
            if let Some(value) = google.get(key).filter(|value| !value.is_null()) {
                parameters.insert(key.to_owned(), value.clone());
            }
        }
        for (key, value) in &google {
            if !CONSUMED_OPTION_KEYS.contains(&key.as_str()) {
                parameters.insert(key.clone(), value.clone());
            }
        }
        PreparedVideoRequest {
            body: json!({"instances": [instance], "parameters": parameters}),
            warnings,
        }
    }

    fn response_metadata(&self, headers: ferrin_spec::Headers) -> ResponseMetadata {
        ResponseMetadata {
            id: None,
            timestamp: Some(chrono::Utc::now()),
            model_id: Some(self.model_id.clone()),
            headers: Some(headers),
            body: None,
        }
    }

    fn completed(
        &self,
        operation: &Operation,
        headers: ferrin_spec::Headers,
    ) -> Result<VideoStatusResult, ProviderError> {
        let samples = operation
            .response
            .as_ref()
            .and_then(|response| response.get("generateVideoResponse"))
            .and_then(|response| response.get("generatedSamples"))
            .and_then(JsonValue::as_array)
            .filter(|samples| !samples.is_empty())
            .ok_or_else(|| {
                ProviderError::InvalidResponseData(Box::new(InvalidResponseDataError::new(
                    "no videos in the video generation response",
                    operation.response.clone().unwrap_or(JsonValue::Null),
                )))
            })?;
        let api_key = self
            .config
            .headers(&ferrin_spec::Headers::new())
            .ok()
            .and_then(|headers| {
                headers
                    .get_str(crate::config::API_KEY_HEADER)
                    .map(|key| secrecy::SecretString::from(key.to_owned()))
            });
        let mut videos = Vec::new();
        let mut metadata = Vec::new();
        for sample in samples {
            let Some(uri) = sample
                .get("video")
                .and_then(|video| video.get("uri"))
                .and_then(JsonValue::as_str)
            else {
                continue;
            };
            let Ok(mut url) = Url::parse(uri) else {
                continue;
            };
            if let Some(key) = &api_key
                && is_same_origin(&url, &self.config.base_url)
            {
                url.query_pairs_mut()
                    .append_pair("key", key.expose_secret());
            }
            videos.push(VideoData {
                data: FileData::Url { url },
                media_type: MediaType::new("video/mp4"),
            });
            metadata.push(json!({"uri": uri}));
        }
        if videos.is_empty() {
            return Err(ProviderError::InvalidResponseData(Box::new(
                InvalidResponseDataError::new(
                    "no valid videos in the video generation response",
                    operation.response.clone().unwrap_or(JsonValue::Null),
                ),
            )));
        }
        let mapper = OutputMapper::new(self.config.clone(), Default::default());
        let mut object = JsonObject::new();
        object.insert("videos".to_owned(), JsonValue::Array(metadata));
        Ok(VideoStatusResult::Completed {
            videos,
            warnings: Vec::new(),
            provider_metadata: Some(mapper.metadata(object)),
            response: self.response_metadata(headers),
        })
    }
}

impl VideoModel for GoogleVideoModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    fn max_videos_per_call(&self) -> Option<usize> {
        Some(MAX_VIDEOS_PER_CALL)
    }

    async fn do_generate(&self, options: VideoOptions) -> Result<VideoResult, ProviderError> {
        let _ = options;
        Err(ProviderError::unsupported(
            "synchronous video generation; use do_start and do_status",
        ))
    }

    fn supports_operations(&self) -> bool {
        true
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_start(
        &self,
        options: VideoStartOptions,
    ) -> Result<VideoStartResult, ProviderError> {
        let mut prepared = self.prepare_request(&options.options);
        if options.webhook_url.is_some() {
            prepared.warnings.push(Warning::unsupported("webhookUrl"));
        }
        let handlers = ResponseHandlers::new(
            json_response_handler::<Operation>(),
            failed_response_handler(),
        );
        let response = post_json(
            self.config.transport.as_ref(),
            self.config
                .model_url(self.model_id.as_str(), "predictLongRunning"),
            self.config.headers(&options.options.headers)?,
            &prepared.body,
            &handlers,
            options.options.cancellation.clone(),
        )
        .await?;
        let name = response.value.name.clone().ok_or_else(|| {
            ProviderError::InvalidResponseData(Box::new(InvalidResponseDataError::new(
                "no operation name returned from the video generation API",
                response.raw.clone().unwrap_or(JsonValue::Null),
            )))
        })?;
        Ok(VideoStartResult {
            operation: json!({"operationName": name}),
            warnings: prepared.warnings,
            provider_metadata: None,
            response: self.response_metadata(response.response_headers),
        })
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_status(
        &self,
        options: VideoStatusOptions,
    ) -> Result<VideoStatusResult, ProviderError> {
        let name = options
            .operation
            .get("operationName")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| {
                ProviderError::InvalidArgument(ferrin_spec::error::InvalidArgumentError::new(
                    "operation",
                    "operation must contain an `operationName` string",
                ))
            })?;
        let handlers = ResponseHandlers::new(
            json_response_handler::<Operation>(),
            failed_response_handler(),
        );
        let response = get(
            self.config.transport.as_ref(),
            self.config.url(name),
            self.config.headers(&options.headers)?,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let operation = response.value;
        if operation.done != Some(true) {
            return Ok(VideoStatusResult::Pending {
                warnings: Vec::new(),
                provider_metadata: None,
                response: self.response_metadata(response.response_headers),
            });
        }
        if let Some(error) = &operation.error {
            return Ok(VideoStatusResult::Error {
                error: format!(
                    "Video generation failed: {}",
                    error.message.as_deref().unwrap_or("unknown error")
                ),
                provider_metadata: None,
                response: self.response_metadata(response.response_headers),
            });
        }
        self.completed(&operation, response.response_headers)
    }
}

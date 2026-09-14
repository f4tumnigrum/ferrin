//! Gemini language model (`generateContent` / `streamGenerateContent`).

use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::event_source_response_handler;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_provider_util::stream_driver::drive_stream;
use ferrin_provider_util::tool_name_mapping::ToolNameMapping;
use ferrin_spec::Content;
use ferrin_spec::FinishReason;
use ferrin_spec::FinishReasonKind;
use ferrin_spec::JsonValue;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderId;
use ferrin_spec::ResponseMetadata;
use ferrin_spec::error::ProviderError;
use ferrin_spec::language_model::CallOptions;
use ferrin_spec::language_model::GenerateResult;
use ferrin_spec::language_model::LanguageModel;
use ferrin_spec::language_model::RequestMetadata;
use ferrin_spec::language_model::StreamPart;
use ferrin_spec::language_model::StreamResult;
use ferrin_spec::language_model::SupportedUrls;
use ferrin_spec::shared::Warning;
use regex::Regex;
use url::Url;

use crate::api_types::GenerateContentResponse;
use crate::capabilities::capabilities;
use crate::config::SharedConfig;
use crate::error::failed_response_handler;
use crate::output::OutputMapper;
use crate::output::convert_usage;
use crate::output::has_client_tool_calls;
use crate::output::map_finish_reason;
use crate::output::raw_usage;
use crate::request::PreparedRequest;
use crate::request::prepare_request;
use crate::stream::GoogleStreamState;

/// Provider id family of the language model.
pub const FAMILY: &str = "generative-ai";

/// Media types the Gemini API fetches from arbitrary HTTPS URLs.
pub const EXTERNAL_URL_MEDIA_TYPES: [&str; 22] = [
    "text/html",
    "text/css",
    "text/plain",
    "text/xml",
    "text/csv",
    "text/rtf",
    "text/javascript",
    "application/json",
    "application/pdf",
    "image/bmp",
    "image/jpeg",
    "image/png",
    "image/webp",
    "video/mp4",
    "video/mpeg",
    "video/quicktime",
    "video/avi",
    "video/x-flv",
    "video/mpg",
    "video/webm",
    "video/wmv",
    "video/3gpp",
];

fn regex(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

/// URLs every Gemini request accepts: Files API URIs (public endpoint and
/// the configured base URL) and YouTube videos.
#[must_use]
pub fn base_supported_urls(base_url: &Url) -> SupportedUrls {
    let escaped_base = regex::escape(base_url.as_str().trim_end_matches('/'));
    let patterns = [
        r"^https://generativelanguage\.googleapis\.com/v1beta/files/.*$".to_owned(),
        format!("^{escaped_base}/files/.*$"),
        r"^https://(?:www\.)?youtube\.com/watch\?v=[\w-]+(?:&[\w=&.-]*)?$".to_owned(),
        r"^https://youtu\.be/[\w-]+(?:\?[\w=&.-]*)?$".to_owned(),
    ];
    SupportedUrls::none().with("*", patterns.iter().filter_map(|pattern| regex(pattern)))
}

/// Supported URLs of `model_id`: the base set plus, for Gemini models other
/// than Gemini 2.0, any HTTPS URL for [`EXTERNAL_URL_MEDIA_TYPES`].
#[must_use]
pub fn supported_urls(base_url: &Url, model_id: &str) -> SupportedUrls {
    let mut urls = base_supported_urls(base_url);
    let lower = model_id.to_ascii_lowercase();
    let is_gemini = lower
        .split('/')
        .any(|segment| segment.starts_with("gemini-"));
    let is_gemini_2_0 = lower
        .split('/')
        .any(|segment| segment.starts_with("gemini-2.0"));
    if is_gemini && !is_gemini_2_0 {
        for media_type in EXTERNAL_URL_MEDIA_TYPES {
            urls.insert(media_type, regex("^https://.*$"));
        }
    }
    urls
}

/// Language model backed by `generateContent`.
#[derive(Debug, Clone)]
pub struct GoogleLanguageModel {
    config: SharedConfig,
    provider: ProviderId,
    model_id: ModelId,
}

impl GoogleLanguageModel {
    /// Creates the model.
    #[must_use]
    pub fn new(config: SharedConfig, model_id: impl Into<ModelId>) -> Self {
        Self {
            provider: config.provider_id(FAMILY),
            config,
            model_id: model_id.into(),
        }
    }

    /// Shared configuration.
    #[must_use]
    pub fn config(&self) -> &SharedConfig {
        &self.config
    }

    /// Builds the request body and warnings for `options` without sending.
    ///
    /// # Errors
    ///
    /// See [`prepare_request`].
    pub fn prepare_request(&self, options: &CallOptions) -> Result<PreparedRequest, ProviderError> {
        prepare_request(&self.config, self.model_id.as_str(), options)
    }

    /// Converts a parsed `generateContent` response (`raw` is the original
    /// JSON) into a result. Used for direct calls and batch results.
    ///
    /// # Errors
    ///
    /// See [`convert_generate_content_response`].
    pub fn convert_response(
        &self,
        prepared: &PreparedRequest,
        body: &GenerateContentResponse,
        raw: Option<&JsonValue>,
    ) -> Result<GenerateResult, ProviderError> {
        convert_generate_content_response(
            &self.config,
            prepared.tool_name_mapping.clone(),
            prepared.warnings.clone(),
            body,
            raw,
        )
    }
}

/// Converts a parsed `generateContent` response (`raw` is the original JSON)
/// into a result using `mapping` to restore custom tool names; `warnings`
/// are attached to the result.
///
/// # Errors
///
/// Returns [`ProviderError::InvalidResponseData`] for undecodable inline
/// data.
pub fn convert_generate_content_response(
    config: &SharedConfig,
    mapping: ToolNameMapping,
    warnings: Vec<Warning>,
    body: &GenerateContentResponse,
    raw: Option<&JsonValue>,
) -> Result<GenerateResult, ProviderError> {
    let mut mapper = OutputMapper::new(config.clone(), mapping);
    let candidate = body.candidate();
    let mut content = Vec::new();
    if let Some(candidate) = candidate {
        content = mapper.map_parts(candidate.parts())?;
        content.extend(
            mapper
                .sources(&candidate.grounding_chunks())
                .into_iter()
                .map(Content::Source),
        );
    }
    let block_reason = body.block_reason();
    let candidate_reason = candidate.and_then(|candidate| candidate.finish_reason.as_deref());
    let finish_reason = match (candidate_reason, block_reason) {
        (None, Some(reason)) => FinishReason::with_raw(FinishReasonKind::ContentFilter, reason),
        (reason, _) => map_finish_reason(reason, has_client_tool_calls(&content)),
    };
    let raw_usage_object = raw_usage(raw);
    let usage_value = raw_usage_object.clone().map(JsonValue::Object);
    let metadata = mapper.response_metadata(body, candidate, usage_value.as_ref());
    let mut result = GenerateResult::new(content, finish_reason);
    result.usage = convert_usage(body.usage_metadata.as_ref(), raw_usage_object);
    result.provider_metadata = Some(metadata);
    result.warnings = warnings;
    result.response = ResponseMetadata {
        id: body.response_id.clone(),
        timestamp: body
            .create_time
            .as_deref()
            .and_then(|time| chrono::DateTime::parse_from_rfc3339(time).ok())
            .map(|time| time.with_timezone(&chrono::Utc)),
        model_id: body.model_version.clone().map(Into::into),
        headers: None,
        body: raw.cloned(),
    };
    Ok(result)
}

impl LanguageModel for GoogleLanguageModel {
    fn provider(&self) -> &ProviderId {
        &self.provider
    }

    fn model_id(&self) -> &ModelId {
        &self.model_id
    }

    async fn supported_urls(&self) -> SupportedUrls {
        supported_urls(&self.config.base_url, self.model_id.as_str())
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_generate(&self, options: CallOptions) -> Result<GenerateResult, ProviderError> {
        let prepared = self.prepare_request(&options)?;
        let url = self
            .config
            .model_url(self.model_id.as_str(), "generateContent");
        let headers = self.config.headers(&options.headers)?;
        let handlers = ResponseHandlers::new(
            json_response_handler::<GenerateContentResponse>(),
            failed_response_handler(),
        );
        let request_body = JsonValue::Object(prepared.body.clone());
        let response = post_json(
            self.config.transport.as_ref(),
            url,
            headers,
            &request_body,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let mut result =
            self.convert_response(&prepared, &response.value, response.raw.as_ref())?;
        result.request = RequestMetadata::with_body(request_body);
        result.response.headers = Some(response.response_headers);
        Ok(result)
    }

    #[tracing::instrument(skip_all, fields(model = %self.model_id))]
    async fn do_stream(&self, options: CallOptions) -> Result<StreamResult, ProviderError> {
        let prepared = self.prepare_request(&options)?;
        let mut url = self
            .config
            .model_url(self.model_id.as_str(), "streamGenerateContent");
        url.set_query(Some("alt=sse"));
        let headers = self.config.headers(&options.headers)?;
        let handlers = ResponseHandlers::new(
            event_source_response_handler::<GenerateContentResponse>(),
            failed_response_handler(),
        );
        let request_body = JsonValue::Object(prepared.body.clone());
        let response = post_json(
            self.config.transport.as_ref(),
            url,
            headers,
            &request_body,
            &handlers,
            options.cancellation.clone(),
        )
        .await?;
        let mapper = OutputMapper::new(self.config.clone(), prepared.tool_name_mapping.clone());
        let state = GoogleStreamState::new(mapper);
        let stream = drive_stream(
            StreamPart::StreamStart {
                warnings: prepared.warnings,
            },
            response.value,
            state,
            options.include_raw_chunks,
        );
        let mut result = StreamResult::new(stream);
        result.request = RequestMetadata::with_body(request_body);
        result.response = ResponseMetadata::with_headers(response.response_headers);
        Ok(result)
    }
}

/// Whether `model_id` is a Gemini model (used by the image model).
#[must_use]
pub fn is_gemini_model(model_id: &str) -> bool {
    capabilities(model_id).is_gemini
}

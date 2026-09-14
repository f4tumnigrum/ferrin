//! Helpers shared by the streaming language models.
//!
//! The stream driver itself lives in `ferrin_provider_util::stream_driver`;
//! this module adds the OpenAI error mapping and small formatting helpers.

pub(crate) use ferrin_provider_util::stream_driver::EarlyChunk;
pub(crate) use ferrin_provider_util::stream_driver::StreamMachine;
pub(crate) use ferrin_provider_util::stream_driver::drive_stream;

use ferrin_provider_util::ParseResult;
use ferrin_spec::BoxStream;
use ferrin_spec::JsonValue;
use ferrin_spec::error::ApiCallError;
use ferrin_spec::error::ProviderError;
use url::Url;

use crate::error::parse_frame_error;

/// Fails the request when the server reports an error before producing any
/// output, so the caller sees an [`ProviderError::ApiCall`] instead of a
/// stream with only an error part. Buffered chunks are replayed.
///
/// # Errors
///
/// Returns the error frame converted to an [`ProviderError::ApiCall`].
pub(crate) async fn fail_on_early_error<T: Send + 'static>(
    stream: BoxStream<'static, ParseResult<T>>,
    url: &Url,
    classify: impl Fn(&T) -> EarlyChunk,
) -> Result<BoxStream<'static, ParseResult<T>>, ProviderError> {
    let url = url.clone();
    ferrin_provider_util::stream_driver::fail_on_early_error(stream, classify, |_, raw| {
        let frame = error_frame(raw);
        match parse_frame_error(&frame) {
            Some(error) => error.to_api_call_error(url, &frame).into(),
            None => ApiCallError::new("OpenAI stream failed before any output was generated", url)
                .with_status(http::StatusCode::INTERNAL_SERVER_ERROR)
                .with_data(frame.clone())
                .retryable(false)
                .into(),
        }
    })
    .await
}

fn error_frame(raw: &JsonValue) -> JsonValue {
    match raw.get("type").and_then(JsonValue::as_str) {
        Some("response.failed") => raw.clone(),
        _ => raw.get("error").cloned().unwrap_or_else(|| raw.clone()),
    }
}

/// Escapes text for embedding inside a JSON string literal (without the
/// surrounding quotes), used when streaming tool input built from parts.
#[must_use]
pub(crate) fn escape_json_delta(delta: &str) -> String {
    let quoted = JsonValue::String(delta.to_owned()).to_string();
    quoted[1..quoted.len() - 1].to_owned()
}

/// Seconds since the epoch (integer or float) to a timestamp.
#[must_use]
pub(crate) fn timestamp_from_seconds(
    seconds: Option<f64>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    let seconds = seconds?;
    if !seconds.is_finite() {
        return None;
    }
    #[allow(
        clippy::cast_possible_truncation,
        reason = "epoch seconds are far below the i64 range"
    )]
    let whole = seconds.trunc() as i64;
    chrono::DateTime::from_timestamp(whole, 0)
}

//! Errors that provider adapters can return.
//!
//! [`ProviderError`] is the single error type of every adapter method. Each
//! variant wraps a concrete error struct so that callers can match on the
//! failure class and read structured fields. Large payloads are boxed to keep
//! the enum small enough to pass through `Result` cheaply.

mod api_call;
mod config;
mod data;
mod model;

use http::StatusCode;

pub use api_call::ApiCallError;
pub use api_call::default_retryable;
pub use config::InvalidArgumentError;
pub use config::InvalidPromptError;
pub use config::LoadApiKeyError;
pub use config::LoadSettingError;
pub use data::EmptyResponseBodyError;
pub use data::InvalidResponseDataError;
pub use data::JsonParseError;
pub use data::NoContentGeneratedError;
pub use data::TypeValidationContext;
pub use data::TypeValidationError;
pub use model::ModelKind;
pub use model::NoSuchModelError;
pub use model::NoSuchProviderReferenceError;
pub use model::TooManyEmbeddingValuesForCallError;
pub use model::UnsupportedFunctionalityError;

/// Boxed dynamic error used for causes and for [`ProviderError::Other`].
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Error returned by provider adapters.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ProviderError {
    /// An HTTP call to the provider failed.
    #[error(transparent)]
    ApiCall(Box<ApiCallError>),
    /// The provider returned an empty body where one was required.
    #[error(transparent)]
    EmptyResponseBody(#[from] EmptyResponseBodyError),
    /// A call argument is invalid.
    #[error(transparent)]
    InvalidArgument(#[from] InvalidArgumentError),
    /// The prompt cannot be converted for this provider.
    #[error(transparent)]
    InvalidPrompt(Box<InvalidPromptError>),
    /// The response has an unexpected shape.
    #[error(transparent)]
    InvalidResponseData(Box<InvalidResponseDataError>),
    /// Response text is not valid JSON.
    #[error(transparent)]
    JsonParse(Box<JsonParseError>),
    /// The API key could not be loaded.
    #[error(transparent)]
    LoadApiKey(#[from] LoadApiKeyError),
    /// A required setting could not be loaded.
    #[error(transparent)]
    LoadSetting(#[from] LoadSettingError),
    /// The provider produced no content.
    #[error(transparent)]
    NoContentGenerated(#[from] NoContentGeneratedError),
    /// The requested model does not exist.
    #[error(transparent)]
    NoSuchModel(Box<NoSuchModelError>),
    /// A provider reference has no entry for this provider.
    #[error(transparent)]
    NoSuchProviderReference(Box<NoSuchProviderReferenceError>),
    /// Too many values were passed to a single embedding call.
    #[error(transparent)]
    TooManyEmbeddingValues(Box<TooManyEmbeddingValuesForCallError>),
    /// A value failed schema or type validation.
    #[error(transparent)]
    TypeValidation(#[from] TypeValidationError),
    /// The provider or model does not support the requested functionality.
    #[error(transparent)]
    UnsupportedFunctionality(#[from] UnsupportedFunctionalityError),
    /// The call was cancelled through its cancellation token.
    #[error("operation cancelled")]
    Cancelled,
    /// Any other error.
    #[error(transparent)]
    Other(#[from] BoxError),
}

impl ProviderError {
    /// Wraps an arbitrary error in [`ProviderError::Other`].
    #[must_use]
    pub fn other(error: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::Other(Box::new(error))
    }

    /// Creates an [`ProviderError::Other`] from a message.
    #[must_use]
    pub fn message(message: impl Into<String>) -> Self {
        Self::Other(message.into().into())
    }

    /// Creates an [`ProviderError::UnsupportedFunctionality`] error.
    #[must_use]
    pub fn unsupported(functionality: impl Into<String>) -> Self {
        Self::UnsupportedFunctionality(UnsupportedFunctionalityError::new(functionality))
    }

    /// Returns `true` when retrying the call may succeed.
    ///
    /// Only [`ProviderError::ApiCall`] carries retry information; every other
    /// variant is not retryable.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::ApiCall(error) => error.is_retryable,
            _ => false,
        }
    }

    /// Returns the HTTP status code of an API call error.
    #[must_use]
    pub fn status_code(&self) -> Option<StatusCode> {
        match self {
            Self::ApiCall(error) => error.status_code,
            _ => None,
        }
    }

    /// Returns the API call error, if this is [`ProviderError::ApiCall`].
    #[must_use]
    pub fn as_api_call(&self) -> Option<&ApiCallError> {
        match self {
            Self::ApiCall(error) => Some(error),
            _ => None,
        }
    }

    /// Returns a stable, low-cardinality name of the variant for telemetry.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match self {
            Self::ApiCall(_) => "api_call",
            Self::EmptyResponseBody(_) => "empty_response_body",
            Self::InvalidArgument(_) => "invalid_argument",
            Self::InvalidPrompt(_) => "invalid_prompt",
            Self::InvalidResponseData(_) => "invalid_response_data",
            Self::JsonParse(_) => "json_parse",
            Self::LoadApiKey(_) => "load_api_key",
            Self::LoadSetting(_) => "load_setting",
            Self::NoContentGenerated(_) => "no_content_generated",
            Self::NoSuchModel(_) => "no_such_model",
            Self::NoSuchProviderReference(_) => "no_such_provider_reference",
            Self::TooManyEmbeddingValues(_) => "too_many_embedding_values",
            Self::TypeValidation(_) => "type_validation",
            Self::UnsupportedFunctionality(_) => "unsupported_functionality",
            Self::Cancelled => "cancelled",
            Self::Other(_) => "other",
        }
    }
}

macro_rules! boxed_from {
    ($($variant:ident($error:ty)),* $(,)?) => {
        $(
            impl From<$error> for ProviderError {
                fn from(error: $error) -> Self {
                    Self::$variant(Box::new(error))
                }
            }

            impl From<Box<$error>> for ProviderError {
                fn from(error: Box<$error>) -> Self {
                    Self::$variant(error)
                }
            }
        )*
    };
}

boxed_from! {
    ApiCall(ApiCallError),
    InvalidPrompt(InvalidPromptError),
    InvalidResponseData(InvalidResponseDataError),
    JsonParse(JsonParseError),
    NoSuchModel(NoSuchModelError),
    NoSuchProviderReference(NoSuchProviderReferenceError),
    TooManyEmbeddingValues(TooManyEmbeddingValuesForCallError),
}

/// Truncates `text` to at most `max_bytes` bytes on a char boundary, appending
/// a marker when truncation happened.
pub(crate) fn truncate_for_display(text: &str, max_bytes: usize) -> std::borrow::Cow<'_, str> {
    if text.len() <= max_bytes {
        return std::borrow::Cow::Borrowed(text);
    }
    let mut end = max_bytes;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    std::borrow::Cow::Owned(format!("{}... [truncated]", &text[..end]))
}

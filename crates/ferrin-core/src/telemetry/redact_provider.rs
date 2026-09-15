//! Remove provider error bodies and opaque causes from telemetry copies.

use ferrin_spec::JsonValue;
use ferrin_spec::error::ApiCallError;
use ferrin_spec::error::EmptyResponseBodyError;
use ferrin_spec::error::InvalidArgumentError;
use ferrin_spec::error::InvalidPromptError;
use ferrin_spec::error::InvalidResponseDataError;
use ferrin_spec::error::JsonParseError;
use ferrin_spec::error::LoadApiKeyError;
use ferrin_spec::error::LoadSettingError;
use ferrin_spec::error::NoContentGeneratedError;
use ferrin_spec::error::ProviderError;
use ferrin_spec::error::TypeValidationError;
use ferrin_spec::error::UnsupportedFunctionalityError;

use super::TelemetryOptions;
use super::redact::REDACTED;

pub(super) fn redact_provider(error: &ProviderError, options: &TelemetryOptions) -> ProviderError {
    match error {
        ProviderError::ApiCall(error) => {
            let mut recorded = ApiCallError::new(REDACTED, error.url.clone());
            recorded.status_code = error.status_code;
            recorded.is_retryable = error.is_retryable;
            recorded.response_headers = error.response_headers.clone();
            if options.record_inputs {
                recorded.request_body = error.request_body.clone();
            } else {
                recorded.url.set_query(None);
                recorded.url.set_fragment(None);
            }
            if options.record_outputs {
                recorded.response_body = error.response_body.clone();
                recorded.data = error.data.clone();
            }
            recorded.into()
        }
        ProviderError::EmptyResponseBody(_) => EmptyResponseBodyError::new().into(),
        ProviderError::InvalidArgument(error) => {
            InvalidArgumentError::new(&error.argument, REDACTED).into()
        }
        ProviderError::InvalidPrompt(error) => {
            let mut recorded = InvalidPromptError::new(REDACTED);
            if options.record_inputs {
                recorded.prompt = error.prompt.clone();
            }
            recorded.into()
        }
        ProviderError::InvalidResponseData(error) => InvalidResponseDataError::new(
            REDACTED,
            if options.record_outputs {
                error.data.clone()
            } else {
                JsonValue::Null
            },
        )
        .into(),
        ProviderError::JsonParse(_) => ProviderError::JsonParse(Box::new(JsonParseError {
            text: REDACTED.into(),
            cause: REDACTED.into(),
        })),
        ProviderError::TypeValidation(_) => ProviderError::TypeValidation(TypeValidationError {
            value: JsonValue::Null,
            context: None,
            cause: REDACTED.into(),
        }),
        ProviderError::LoadApiKey(_) => LoadApiKeyError::new(REDACTED).into(),
        ProviderError::LoadSetting(_) => LoadSettingError::new(REDACTED).into(),
        ProviderError::NoContentGenerated(_) => NoContentGeneratedError::new().into(),
        ProviderError::NoSuchModel(error) => {
            let mut recorded = (**error).clone();
            recorded.message = REDACTED.into();
            recorded.into()
        }
        ProviderError::NoSuchProviderReference(error) => {
            let mut recorded = (**error).clone();
            recorded.message = REDACTED.into();
            if !options.record_inputs {
                recorded.reference = Default::default();
            }
            recorded.into()
        }
        ProviderError::TooManyEmbeddingValues(error) => (**error).clone().into(),
        ProviderError::UnsupportedFunctionality(error) => {
            UnsupportedFunctionalityError::with_message(&error.functionality, REDACTED).into()
        }
        ProviderError::Cancelled => ProviderError::Cancelled,
        ProviderError::Other(_) => ProviderError::message(REDACTED),
        _ => ProviderError::message(REDACTED),
    }
}

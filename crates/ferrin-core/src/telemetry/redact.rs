//! Telemetry-only error copies: preserve classifications without retaining hidden content.

use ferrin_message::Message;
use ferrin_spec::ResponseMetadata;

use super::TelemetryOptions;
use super::redact_provider::redact_provider;
use crate::error::Error;
use crate::error::NoObjectGeneratedDetails;

pub(super) const REDACTED: &str = "error details omitted by telemetry recording settings";

pub(super) fn response(
    response: &ResponseMetadata,
    options: &TelemetryOptions,
) -> ResponseMetadata {
    let mut recorded = response.clone();
    if !options.record_outputs {
        recorded.body = None;
    }
    recorded
}

pub(super) fn redact_error(error: &Error, options: &TelemetryOptions) -> Error {
    match error {
        Error::Provider(error) => Error::Provider(Box::new(redact_provider(error, options))),
        Error::Retry {
            reason,
            attempts,
            errors,
        } => Error::Retry {
            reason: *reason,
            attempts: *attempts,
            errors: errors
                .iter()
                .map(|error| redact_provider(error, options))
                .collect(),
        },
        Error::Timeout { scope, elapsed } => Error::Timeout {
            scope: scope.clone(),
            elapsed: *elapsed,
        },
        Error::Cancelled => Error::Cancelled,
        Error::InvalidArgument { argument, .. } => Error::invalid_argument(argument, REDACTED),
        Error::InvalidPrompt { .. } => Error::invalid_prompt(REDACTED),
        Error::MessageConversion { .. } => Error::MessageConversion {
            message: REDACTED.into(),
            original_message: Box::new(Message::user(REDACTED)),
        },
        Error::Download(details) => {
            let mut url = details.url.clone();
            if !options.record_inputs {
                url.set_path("/");
                url.set_query(None);
                url.set_fragment(None);
                let _ = url.set_username("");
                let _ = url.set_password(None);
            }
            Error::download(url, details.status_code, None)
        }
        Error::InvalidDataContent { .. } => Error::invalid_data_content(REDACTED, None),
        Error::NoSuchTool {
            tool_name,
            available_tools,
        } => Error::no_such_tool(tool_name.clone(), available_tools.clone()),
        Error::InvalidToolInput(details) => Error::invalid_tool_input(
            details.tool_name.clone(),
            if options.record_inputs && options.record_outputs {
                details.tool_input.clone()
            } else {
                REDACTED.into()
            },
            crate::error::BoxError::from(REDACTED),
        ),
        Error::ToolCallRepair { original, .. } => Error::ToolCallRepair {
            original: Box::new(redact_error(original, options)),
            cause: REDACTED.into(),
        },
        Error::ToolChoiceViolation { expected, actual } => Error::ToolChoiceViolation {
            expected: expected.clone(),
            actual: actual.clone(),
        },
        Error::ToolCallNotFoundForApproval {
            tool_call_id,
            approval_id,
        } => Error::ToolCallNotFoundForApproval {
            tool_call_id: tool_call_id.clone(),
            approval_id: approval_id.clone(),
        },
        Error::ToolChoiceNotSatisfied { expected } => Error::ToolChoiceNotSatisfied {
            expected: expected.clone(),
        },
        Error::InvalidToolApproval { approval_id, .. } => Error::InvalidToolApproval {
            approval_id: approval_id.clone(),
            message: REDACTED.into(),
        },
        Error::NoObjectGenerated(details) => Error::no_object_generated(NoObjectGeneratedDetails {
            message: REDACTED.into(),
            text: options
                .record_outputs
                .then(|| details.text.clone())
                .flatten(),
            response: response(&details.response, options),
            usage: details.usage.clone(),
            finish_reason: details.finish_reason.clone(),
            cause: None,
        }),
        Error::NoOutputGenerated => Error::NoOutputGenerated,
        Error::NoImageGenerated { responses } => Error::NoImageGenerated {
            responses: responses
                .iter()
                .map(|item| response(item, options))
                .collect(),
        },
        Error::NoSpeechGenerated { responses } => Error::NoSpeechGenerated {
            responses: responses
                .iter()
                .map(|item| response(item, options))
                .collect(),
        },
        Error::NoTranscriptGenerated { responses } => Error::NoTranscriptGenerated {
            responses: responses
                .iter()
                .map(|item| response(item, options))
                .collect(),
        },
        Error::NoVideoGenerated { responses } => Error::NoVideoGenerated {
            responses: responses
                .iter()
                .map(|item| response(item, options))
                .collect(),
        },
        Error::NoSuchProvider(details) => Error::NoSuchProvider(details.clone()),
        Error::NoDefaultRegistry { model_id } => Error::NoDefaultRegistry {
            model_id: model_id.clone(),
        },
        Error::InvalidStreamPart { .. } => Error::invalid_stream_part(REDACTED),
        Error::Stream(error) => {
            let mut recorded = (**error).clone();
            recorded.message = REDACTED.into();
            recorded.data = None;
            Error::stream(recorded)
        }
        Error::Mcp(_) => Error::Mcp(REDACTED.into()),
        Error::Other(_) => Error::Other(REDACTED.into()),
    }
}

/// Keep warning categories and feature names while hiding opaque descriptions.
pub(super) fn warnings(warnings: &[ferrin_spec::Warning]) -> Vec<ferrin_spec::Warning> {
    use ferrin_spec::Warning;
    warnings
        .iter()
        .map(|warning| match warning {
            Warning::Unsupported { feature, .. } => Warning::unsupported(feature),
            Warning::Compatibility { feature, .. } => Warning::compatibility(feature, None),
            Warning::Deprecated { setting, .. } => Warning::deprecated(setting, REDACTED),
            Warning::Other { .. } => Warning::other(REDACTED),
            _ => Warning::other(REDACTED),
        })
        .collect()
}

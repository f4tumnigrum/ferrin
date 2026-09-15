//! Bounded streaming reads of server-provided batch result resources.

use ferrin_provider_util::http::Handled;
use ferrin_provider_util::http::HttpResponse;
use ferrin_provider_util::http::ParseResult;
use ferrin_provider_util::http::ResponseContext;
use ferrin_provider_util::http::ResponseHandler;
use ferrin_provider_util::http::TransportError;
use ferrin_provider_util::http::TransportErrorKind;
use ferrin_provider_util::http::json_lines_response_handler;
use ferrin_spec::BoxFuture;
use ferrin_spec::BoxStream;
use ferrin_spec::error::ProviderError;
use futures_util::StreamExt;

use super::results::BatchResultLine;

type Lines = BoxStream<'static, ParseResult<BatchResultLine>>;

pub(super) struct ResultsHandler {
    pub(super) max_bytes: u64,
}

impl ResponseHandler<Lines> for ResultsHandler {
    fn handle(
        &self,
        context: ResponseContext,
        mut response: HttpResponse,
    ) -> BoxFuture<'static, Result<Handled<Lines>, ProviderError>> {
        let limit = self.max_bytes;
        response.body = Box::pin(response.body.scan(
            (0_u64, false),
            move |(count, done), chunk| {
                let next = if *done {
                    None
                } else if let Ok(bytes) = &chunk {
                    if bytes.len() as u64 > limit.saturating_sub(*count) {
                        *done = true;
                        Some(Err(TransportError::new(
                            TransportErrorKind::BodyTooLarge,
                            "batch result body exceeds the configured byte limit",
                        )))
                    } else {
                        *count += bytes.len() as u64;
                        Some(chunk)
                    }
                } else {
                    *done = true;
                    Some(chunk)
                };
                futures_util::future::ready(next)
            },
        ));
        json_lines_response_handler::<BatchResultLine>().handle(context, response)
    }
}

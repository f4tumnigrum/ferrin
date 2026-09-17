//! Voyage HTTP errors.
//!
//! Derived from Vercel AI SDK `packages/voyage/src/voyage-error.ts`
//! (Apache-2.0, Copyright 2023 Vercel, Inc.); translated and modified.

use ferrin_provider_util::http::JsonErrorResponseHandler;
use ferrin_provider_util::http::json_error_response_handler;
use serde::Deserialize;

#[derive(Deserialize)]
pub(crate) struct VoyageError {
    detail: String,
}

pub(crate) fn failed_response_handler() -> JsonErrorResponseHandler<VoyageError> {
    json_error_response_handler::<VoyageError>(|error| error.detail.clone())
}

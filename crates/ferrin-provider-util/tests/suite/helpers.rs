use ferrin_spec::error::ProviderError;

pub(crate) fn api_error(error: &ProviderError) -> &ferrin_spec::error::ApiCallError {
    match error {
        ProviderError::ApiCall(error) => error,
        other => panic!("expected api call error, got {other:?}"),
    }
}

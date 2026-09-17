//! Azure authentication and API-version attachment.
//!
//! Derived from the Vercel AI SDK (Apache-2.0, Copyright 2023 Vercel, Inc.),
//! reimplemented in Rust; see `NOTICE`.

use std::sync::Arc;

use ferrin_provider_util::SharedTransport;
use ferrin_provider_util::http::HttpRequest;
use ferrin_provider_util::http::HttpResponse;
use ferrin_provider_util::http::HttpTransport;
use ferrin_provider_util::http::TransportError;
use ferrin_provider_util::http::TransportErrorKind;
use ferrin_provider_util::settings::ApiKeyConfig;
use ferrin_provider_util::settings::load_api_key;
use ferrin_spec::BoxFuture;
use secrecy::ExposeSecret;
use secrecy::SecretString;
use tokio::time::Instant;
use url::Url;

use crate::TokenProvider;

pub(crate) struct AzureTransport {
    pub(crate) inner: SharedTransport,
    pub(crate) api_key: Option<SecretString>,
    pub(crate) token_provider: Option<Arc<dyn TokenProvider>>,
    pub(crate) base_url: Url,
    pub(crate) api_version: Option<String>,
    pub(crate) valid_deployment: bool,
}

impl AzureTransport {
    fn is_api_url(&self, url: &Url) -> bool {
        let prefix = self.base_url.path().trim_end_matches('/');
        url.origin() == self.base_url.origin()
            && url.username().is_empty()
            && url.password().is_none()
            && url
                .path()
                .strip_prefix(prefix)
                .is_some_and(|rest| rest.starts_with('/'))
            && percent_encoding::percent_decode_str(url.path())
                .decode_utf8()
                .is_ok_and(|path| {
                    !path.contains('\\')
                        && !path.contains('%')
                        && !path.chars().any(char::is_control)
                        && !path.split('/').any(|segment| matches!(segment, "." | ".."))
                })
    }

    async fn send(&self, mut request: HttpRequest) -> Result<HttpResponse, TransportError> {
        let started = Instant::now();
        if !self.valid_deployment {
            return Err(auth_error("invalid azure deployment name"));
        }
        if !self.is_api_url(&request.url) {
            request.headers.remove("authorization");
            request.headers.remove("api-key");
            return self.inner.execute(request).await;
        }
        request.headers.remove("authorization");
        request.headers.remove("api-key");
        let (name, credential) = match &self.token_provider {
            Some(provider) => {
                let token = tokio::select! {
                    biased;
                    _ = request.cancellation.cancelled() => return Err(TransportError::new(TransportErrorKind::Cancelled, "azure token acquisition cancelled")),
                    token = provider.token() => token.map_err(|_| auth_error("azure token acquisition failed"))?,
                };
                ("authorization", format!("Bearer {}", token.expose_secret()))
            }
            None => {
                let key = load_api_key(ApiKeyConfig {
                    api_key: self.api_key.clone(),
                    environment_variable: "AZURE_API_KEY",
                    parameter_name: "api_key",
                    description: "Azure OpenAI",
                })
                .map_err(|_| auth_error("azure API key is missing"))?;
                ("api-key", key.expose_secret().to_owned())
            }
        };
        request
            .headers
            .insert(name, &credential)
            .map_err(|_| auth_error("invalid azure credential header"))?;
        request.headers = request
            .headers
            .with_user_agent_suffix([concat!("ferrin-azure/", env!("CARGO_PKG_VERSION"))]);
        if let Some(version) = &self.api_version {
            let query: Vec<(String, String)> = request
                .url
                .query_pairs()
                .filter(|(key, _)| key != "api-version")
                .map(|(key, value)| (key.into_owned(), value.into_owned()))
                .collect();
            request.url.set_query(None);
            request
                .url
                .query_pairs_mut()
                .extend_pairs(query)
                .append_pair("api-version", version);
        }
        // The inner transport also owns the response body deadline, so token
        // acquisition must consume its budget rather than restarting it.
        if let Some(timeout) = request.timeout {
            request.timeout = Some(timeout.checked_sub(started.elapsed()).ok_or_else(|| {
                TransportError::new(TransportErrorKind::Timeout, "azure request timed out")
            })?);
        }
        self.inner.execute(request).await
    }
}

fn auth_error(message: &str) -> TransportError {
    TransportError::new(TransportErrorKind::InvalidRequest, message)
}

impl HttpTransport for AzureTransport {
    fn execute(&self, request: HttpRequest) -> BoxFuture<'_, Result<HttpResponse, TransportError>> {
        Box::pin(async move {
            let timeout = request.timeout;
            let cancellation = request.cancellation.clone();
            tokio::select! {
                biased;
                () = cancellation.cancelled() => Err(TransportError::new(TransportErrorKind::Cancelled, "azure request cancelled")),
                result = async {
                    match timeout {
                        Some(timeout) => tokio::time::timeout(timeout, self.send(request))
                            .await
                            .map_err(|_| {
                                TransportError::new(TransportErrorKind::Timeout, "azure request timed out")
                            })?,
                        None => self.send(request).await,
                    }
                } => result,
            }
        })
    }
}

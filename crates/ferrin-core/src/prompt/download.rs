//! URL downloads for file parts the model cannot fetch itself.

use std::fmt;
use std::sync::Arc;

use bytes::Bytes;
use ferrin_provider_util::SharedTransport;
use ferrin_provider_util::UrlPolicy;
use ferrin_provider_util::secure_url::DownloadError;
use ferrin_provider_util::secure_url::DownloadErrorKind;
use ferrin_spec::BoxFuture;
use ferrin_spec::MediaType;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::error::Error;
use crate::limits::DEFAULT_MAX_PARALLEL_DOWNLOADS;

/// One URL to download.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadRequest {
    /// The URL.
    pub url: Url,
    /// Whether the model can fetch the URL itself (matched against
    /// `supported_urls`).
    pub is_url_supported_by_model: bool,
}

/// A downloaded file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DownloadedFile {
    /// The bytes.
    pub data: Bytes,
    /// Media type from the response headers, if any.
    pub media_type: Option<MediaType>,
}

/// Downloads files referenced by URL.
///
/// Implementations return one entry per request in the same order; `None`
/// keeps the URL in the prompt for the provider to fetch.
pub trait DownloadFn: Send + Sync {
    /// Downloads `requests`.
    fn download(
        &self,
        requests: Vec<DownloadRequest>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'_, Result<Vec<Option<DownloadedFile>>, Error>>;
}

/// The default downloader: fetches only URLs the model does not support,
/// through the secure URL policy (HTTPS, no private networks, size limit).
#[derive(Clone)]
pub struct DefaultDownloader {
    transport: SharedTransport,
    policy: Arc<UrlPolicy>,
    max_parallel: usize,
}

impl DefaultDownloader {
    /// Creates a downloader on `transport` with the default policy.
    #[must_use]
    pub fn new(transport: SharedTransport) -> Self {
        Self {
            transport,
            policy: Arc::new(UrlPolicy::default()),
            max_parallel: DEFAULT_MAX_PARALLEL_DOWNLOADS,
        }
    }

    /// Creates a downloader on the default HTTP transport.
    ///
    /// # Errors
    ///
    /// Returns [`Error::Other`] when the transport cannot be built.
    pub fn try_default() -> Result<Self, Error> {
        let transport = ferrin_provider_util::default_transport().map_err(Error::other)?;
        Ok(Self::new(transport))
    }

    /// Replaces the URL policy.
    #[must_use]
    pub fn with_policy(mut self, policy: UrlPolicy) -> Self {
        self.policy = Arc::new(policy);
        self
    }

    /// Sets the maximum number of concurrent downloads (at least 1).
    #[must_use]
    pub fn with_max_parallel(mut self, max_parallel: usize) -> Self {
        self.max_parallel = max_parallel.max(1);
        self
    }

    async fn fetch_one(
        transport: SharedTransport,
        policy: Arc<UrlPolicy>,
        url: Url,
        cancellation: CancellationToken,
    ) -> Result<DownloadedFile, Error> {
        let downloaded =
            ferrin_provider_util::secure_url::fetch(transport.as_ref(), url, &policy, cancellation)
                .await
                .map_err(download_error)?;
        Ok(DownloadedFile {
            data: downloaded.data,
            media_type: downloaded.media_type,
        })
    }
}

impl fmt::Debug for DefaultDownloader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DefaultDownloader")
            .field("policy", &self.policy)
            .field("max_parallel", &self.max_parallel)
            .finish_non_exhaustive()
    }
}

impl DownloadFn for DefaultDownloader {
    fn download(
        &self,
        requests: Vec<DownloadRequest>,
        cancellation: CancellationToken,
    ) -> BoxFuture<'_, Result<Vec<Option<DownloadedFile>>, Error>> {
        Box::pin(async move {
            let mut results: Vec<Option<DownloadedFile>> = vec![None; requests.len()];
            let mut pending = requests
                .into_iter()
                .enumerate()
                .filter(|(_, request)| !request.is_url_supported_by_model);
            let mut tasks: JoinSet<(usize, Result<DownloadedFile, Error>)> = JoinSet::new();
            let mut spawn_next = |tasks: &mut JoinSet<(usize, Result<DownloadedFile, Error>)>| {
                if let Some((index, request)) = pending.next() {
                    let transport = Arc::clone(&self.transport);
                    let policy = Arc::clone(&self.policy);
                    let cancellation = cancellation.clone();
                    tasks.spawn(async move {
                        let result =
                            Self::fetch_one(transport, policy, request.url, cancellation).await;
                        (index, result)
                    });
                    true
                } else {
                    false
                }
            };
            for _ in 0..self.max_parallel {
                if !spawn_next(&mut tasks) {
                    break;
                }
            }
            while let Some(joined) = tasks.join_next().await {
                let (index, result) = joined
                    .map_err(|error| Error::message(format!("download task failed: {error}")))?;
                match result {
                    Ok(file) => results[index] = Some(file),
                    Err(error) => {
                        tasks.abort_all();
                        return Err(error);
                    }
                }
                spawn_next(&mut tasks);
            }
            Ok(results)
        })
    }
}

fn download_error(error: DownloadError) -> Error {
    if error.is_cancelled() {
        return Error::Cancelled;
    }
    let url = error.url().clone();
    let status_code = match error.kind() {
        DownloadErrorKind::Status { status, .. } => Some(*status),
        _ => None,
    };
    Error::download(url, status_code, Some(Box::new(error)))
}

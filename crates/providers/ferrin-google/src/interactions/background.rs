//! Resumable background SSE, derived from the Vercel AI SDK
//! (Apache-2.0, Copyright 2023 Vercel, Inc.), translated and modified; see `NOTICE`.

use std::time::Duration;

use ferrin_provider_util::http::ParseResult;
use ferrin_provider_util::http::ResponseHandlers;
use ferrin_provider_util::http::event_source_response_handler;
use ferrin_provider_util::http::get;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_spec::BoxStream;
use ferrin_spec::Headers;
use ferrin_spec::JsonValue;
use ferrin_spec::error::ProviderError;
use futures_util::StreamExt;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::config::SharedConfig;
use crate::error::failed_response_handler;

pub(super) fn url(config: &SharedConfig, id: &str) -> Result<Url, ProviderError> {
    if matches!(id, "" | "." | "..") {
        return Err(super::invalid(
            "interaction_id",
            "interaction id must be a nonempty resource identifier",
        ));
    }
    let mut url = config.url("interactions");
    url.path_segments_mut()
        .map_err(|()| super::bad_response("invalid interactions endpoint"))?
        .push(id);
    Ok(url)
}

pub(super) async fn cancel(
    config: &SharedConfig,
    id: &str,
    headers: Headers,
    cancellation: CancellationToken,
) -> Result<JsonValue, ProviderError> {
    let mut url = url(config, id)?;
    url.path_segments_mut()
        .map_err(|()| super::bad_response("invalid interactions endpoint"))?
        .push("cancel");
    let handlers = ResponseHandlers::new(
        json_response_handler::<JsonValue>(),
        failed_response_handler(),
    );
    Ok(post_json(
        config.transport.as_ref(),
        url,
        headers,
        &json!({}),
        &handlers,
        cancellation,
    )
    .await?
    .value)
}

struct Resume {
    config: SharedConfig,
    id: String,
    headers: Headers,
    cancellation: CancellationToken,
    deadline: tokio::time::Instant,
    current: Option<BoxStream<'static, ParseResult<JsonValue>>>,
    last_event_id: Option<String>,
    attempts: u8,
    terminal: bool,
}

pub(super) fn stream(
    config: SharedConfig,
    id: String,
    headers: Headers,
    cancellation: CancellationToken,
    timeout_ms: u64,
) -> BoxStream<'static, ParseResult<JsonValue>> {
    let state = Resume {
        config,
        id,
        headers,
        cancellation,
        deadline: tokio::time::Instant::now() + Duration::from_millis(timeout_ms),
        current: None,
        last_event_id: None,
        attempts: 0,
        terminal: false,
    };
    Box::pin(futures_util::stream::unfold(
        state,
        |mut state| async move {
            if state.terminal {
                return None;
            }
            let cancellation = state.cancellation.clone();
            let next = tokio::select! {
                biased;
                () = cancellation.cancelled() => Err(ProviderError::Cancelled),
                result = tokio::time::timeout_at(state.deadline, state.next()) => result.unwrap_or_else(|_| Err(super::bad_response("background interactions streaming timed out"))),
            };
            let result = match next {
                Ok(value) => value,
                Err(error) => {
                    // Explicit abort cancels the remote run; drop remains task-free and
                    // callers can use cancel_interaction for remote cleanup after drop.
                    if matches!(error, ProviderError::Cancelled) {
                        let _ = tokio::time::timeout(
                            Duration::from_secs(2),
                            cancel(
                                &state.config,
                                &state.id,
                                state.headers.clone(),
                                CancellationToken::new(),
                            ),
                        )
                        .await;
                    }
                    state.terminal = true;
                    ParseResult::Err { error, raw: None }
                }
            };
            Some((result, state))
        },
    ))
}

impl Resume {
    async fn next(&mut self) -> Result<ParseResult<JsonValue>, ProviderError> {
        loop {
            if self.current.is_none() {
                let mut endpoint = url(&self.config, &self.id)?;
                endpoint.query_pairs_mut().append_pair("stream", "true");
                if let Some(id) = &self.last_event_id {
                    endpoint.query_pairs_mut().append_pair("last_event_id", id);
                }
                let handlers = ResponseHandlers::new(
                    event_source_response_handler::<JsonValue>(),
                    failed_response_handler(),
                );
                match get(
                    self.config.transport.as_ref(),
                    endpoint,
                    self.headers.clone().with("accept", "text/event-stream"),
                    &handlers,
                    self.cancellation.clone(),
                )
                .await
                {
                    Ok(response) => self.current = Some(response.value),
                    Err(error) => {
                        self.retry(error).await?;
                        continue;
                    }
                }
            }
            let next = match self.current.as_mut() {
                Some(stream) => stream.next().await,
                None => continue,
            };
            match next {
                Some(ParseResult::Ok { value, raw }) => {
                    let id = value["event_id"].as_str().filter(|id| !id.is_empty());
                    // Providers may replay the resume boundary; never duplicate a delta.
                    if id.is_some() && id == self.last_event_id.as_deref() {
                        continue;
                    }
                    if let Some(id) = id {
                        self.last_event_id = Some(id.to_owned());
                    }
                    if matches!(
                        value["event_type"].as_str(),
                        Some("interaction.completed" | "interaction.complete" | "error")
                    ) {
                        self.terminal = true;
                    }
                    return Ok(ParseResult::Ok { value, raw });
                }
                Some(ParseResult::Err { error, raw }) => {
                    if matches!(error, ProviderError::Cancelled) {
                        return Err(error);
                    }
                    if self.last_event_id.is_some()
                        && matches!(error, ProviderError::ApiCall(_))
                        && error.is_retryable()
                    {
                        self.current = None;
                        self.retry(error).await?;
                        continue;
                    }
                    // JSON/schema failures must not be hidden by transport reconnection.
                    self.terminal = true;
                    return Ok(ParseResult::Err { error, raw });
                }
                None => {
                    self.current = None;
                    if self.last_event_id.is_none() {
                        return Err(super::bad_response(
                            "background stream ended without a resume event id",
                        ));
                    }
                    self.retry(super::bad_response(
                        "background interactions retry limit exceeded",
                    ))
                    .await?;
                }
            }
        }
    }

    async fn retry(&mut self, error: ProviderError) -> Result<(), ProviderError> {
        self.attempts = self.attempts.saturating_add(1);
        if self.attempts >= 3 || matches!(error, ProviderError::Cancelled) {
            return Err(error);
        }
        tokio::time::sleep(Duration::from_millis(500 * u64::from(self.attempts))).await;
        Ok(())
    }
}

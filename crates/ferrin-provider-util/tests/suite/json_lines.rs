//! JSON Lines byte limits, chunk boundaries and prompt body release.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Poll;

use bytes::Bytes;
use ferrin_provider_util::http::HttpResponse;
use ferrin_provider_util::http::ParseResult;
use ferrin_provider_util::http::ResponseContext;
use ferrin_provider_util::http::ResponseHandler;
use ferrin_provider_util::http::json_lines_response_handler;
use ferrin_spec::Headers;
use ferrin_spec::error::ProviderError;
use futures_util::StreamExt;
use http::StatusCode;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use url::Url;

fn context() -> ResponseContext {
    ResponseContext::new(Url::parse("https://api.example.com/results").unwrap(), None)
}

struct DropNotice(Arc<AtomicBool>);

impl Drop for DropNotice {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn oversized_json_line_ends_stream_and_releases_body_immediately() {
    for chunks in [
        vec!["12345"],
        vec!["12", "34", "5"],
        vec!["     "],
        vec!["12345\n"],
    ] {
        let dropped = Arc::new(AtomicBool::new(false));
        let reads = Arc::new(AtomicUsize::new(0));
        let notice = DropNotice(Arc::clone(&dropped));
        let polls = Arc::clone(&reads);
        let expected_reads = chunks.len();
        let mut chunks: VecDeque<_> = chunks
            .into_iter()
            .map(|text| Bytes::from_static(text.as_bytes()))
            .collect();
        let body = futures_util::stream::poll_fn(move |_| {
            let _keep_alive = &notice;
            polls.fetch_add(1, Ordering::SeqCst);
            match chunks.pop_front() {
                Some(bytes) => Poll::Ready(Some(Ok(bytes))),
                None => Poll::Pending,
            }
        });
        let response = HttpResponse::from_stream(StatusCode::OK, Headers::new(), Box::pin(body));
        let mut stream = json_lines_response_handler::<Value>()
            .with_max_line_bytes(4)
            .handle(context(), response)
            .await
            .unwrap()
            .value;
        let item = tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())
            .await
            .unwrap()
            .unwrap();
        let ParseResult::Err {
            error: ProviderError::ApiCall(error),
            raw,
        } = item
        else {
            panic!("expected bounded response error");
        };
        assert!(!error.is_retryable);
        assert!(error.message.contains("limit of 4 bytes"));
        assert_eq!(
            (
                raw,
                reads.load(Ordering::SeqCst),
                dropped.load(Ordering::SeqCst)
            ),
            (None, expected_reads, true)
        );
        assert!(stream.next().await.is_none());
    }
}

#[tokio::test]
async fn json_lines_limits_apply_per_line_across_every_chunk_boundary() {
    let input = b"{\"v\":1}\r\n{\"v\":2}\n\nnull";
    for boundary in 0..=input.len() {
        let body = futures_util::stream::iter([
            Ok(Bytes::copy_from_slice(&input[..boundary])),
            Ok(Bytes::copy_from_slice(&input[boundary..])),
        ]);
        let response = HttpResponse::from_stream(StatusCode::OK, Headers::new(), Box::pin(body));
        let stream = json_lines_response_handler::<Value>()
            .with_max_line_bytes(8)
            .handle(context(), response)
            .await
            .unwrap()
            .value;
        let values: Vec<_> = stream
            .map(|item| item.into_result().unwrap())
            .collect()
            .await;
        assert_eq!(values, vec![json!({"v":1}), json!({"v":2}), Value::Null]);
    }
}

#[tokio::test]
async fn json_lines_preserve_prior_values_and_accept_exact_unterminated_limit() {
    for (input, expected, errors) in [
        ("12\n34\n567\n89\n", vec![json!(12), json!(34)], 1),
        ("12", vec![json!(12)], 0),
    ] {
        let response = HttpResponse::from_bytes(
            StatusCode::OK,
            Headers::new(),
            Bytes::copy_from_slice(input.as_bytes()),
        );
        let mut stream = json_lines_response_handler::<Value>()
            .with_max_line_bytes(2)
            .handle(context(), response)
            .await
            .unwrap()
            .value;
        let mut values = Vec::new();
        let mut failed = 0;
        while let Some(item) = stream.next().await {
            match item.into_result() {
                Ok(value) => values.push(value),
                Err(_) => failed += 1,
            }
        }
        assert_eq!((values, failed), (expected, errors));
    }
}

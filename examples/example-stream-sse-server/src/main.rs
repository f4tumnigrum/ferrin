//! A minimal hyper 1.x server that streams `StreamEvent`s to browsers as
//! Server-Sent Events.
//!
//! ```text
//! OPENAI_API_KEY=... cargo run -p example-stream-sse-server
//! curl -N 'http://127.0.0.1:3000/chat?prompt=Tell+me+a+haiku+about+Rust'
//! ```
//!
//! Every event is one `data:` line holding the JSON form of the event; the
//! stream ends with `data: [DONE]`.

#![allow(clippy::print_stdout, clippy::print_stderr)]

use std::convert::Infallible;
use std::net::SocketAddr;

use bytes::Bytes;
use ferrin::openai::OpenAiSettings;
use ferrin::openai::create_openai;
use ferrin::prelude::*;
use ferrin::provider_util::settings::env_var;
use futures_util::stream;
use http_body_util::BodyExt;
use http_body_util::Full;
use http_body_util::StreamBody;
use http_body_util::combinators::UnsyncBoxBody;
use hyper::Method;
use hyper::Request;
use hyper::Response;
use hyper::StatusCode;
use hyper::body::Frame;
use hyper::body::Incoming;
use hyper::header;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::task::JoinSet;

type Body = UnsyncBoxBody<Bytes, Infallible>;

fn text(status: StatusCode, message: impl Into<Bytes>) -> Response<Body> {
    let mut response = Response::new(Full::new(message.into()).boxed_unsync());
    *response.status_mut() = status;
    response
}

/// One SSE frame: `data: <json>\n\n`.
fn sse_frame(event: &StreamEvent) -> Frame<Bytes> {
    let json = ferrin::serde_json::to_string(event)
        .unwrap_or_else(|error| format!("{{\"type\":\"error\",\"message\":\"{error}\"}}"));
    Frame::data(Bytes::from(format!("data: {json}\n\n")))
}

fn prompt_from_query(query: Option<&str>) -> Option<String> {
    url::form_urlencoded::parse(query?.as_bytes())
        .find(|(key, _)| key == "prompt")
        .map(|(_, value)| value.into_owned())
}

async fn handle(
    request: Request<Incoming>,
    model: LanguageModelRef,
) -> Result<Response<Body>, Infallible> {
    if request.method() != Method::GET || request.uri().path() != "/chat" {
        return Ok(text(StatusCode::NOT_FOUND, "try GET /chat?prompt=..."));
    }
    let Some(prompt) = prompt_from_query(request.uri().query()) else {
        return Ok(text(
            StatusCode::BAD_REQUEST,
            "missing `prompt` query parameter",
        ));
    };
    let result = match stream_text(model).prompt(prompt).await {
        Ok(result) => result,
        Err(error) => {
            eprintln!("stream_text failed: {error}");
            return Ok(text(
                StatusCode::BAD_GATEWAY,
                format!("model call failed: {error}"),
            ));
        }
    };
    // The completion handle is not needed here: the event stream itself
    // carries `finish` and `error` events.
    let (events, _completion) = result.split();
    let frames = events
        .map(|event| Ok::<_, Infallible>(sse_frame(&event)))
        .chain(stream::once(async {
            Ok(Frame::data(Bytes::from_static(b"data: [DONE]\n\n")))
        }));
    let response = Response::builder()
        .header(header::CONTENT_TYPE, "text/event-stream")
        .header(header::CACHE_CONTROL, "no-cache")
        .header("x-accel-buffering", "no")
        .body(StreamBody::new(frames).boxed_unsync());
    Ok(response.unwrap_or_else(|error| {
        text(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("invalid response: {error}"),
        )
    }))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let openai = create_openai(OpenAiSettings::default())?;
    let model_id = env_var("OPENAI_MODEL").unwrap_or_else(|| "gpt-5".to_owned());
    let model: LanguageModelRef = openai.responses(&model_id).into();

    let addr: SocketAddr = env_var("SSE_ADDR")
        .unwrap_or_else(|| "127.0.0.1:3000".to_owned())
        .parse()?;
    let listener = TcpListener::bind(addr).await?;
    println!("listening on http://{addr}/chat?prompt=...");

    // Connections run as tasks owned by this JoinSet, so they are cancelled
    // together with the server.
    let mut connections = JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, peer) = accepted?;
                let model = model.clone();
                connections.spawn(async move {
                    let service = service_fn(move |request| handle(request, model.clone()));
                    if let Err(error) = http1::Builder::new()
                        .serve_connection(TokioIo::new(stream), service)
                        .await
                    {
                        eprintln!("connection from {peer} failed: {error}");
                    }
                });
            }
            Some(joined) = connections.join_next(), if !connections.is_empty() => {
                if let Err(error) = joined {
                    eprintln!("connection task failed: {error}");
                }
            }
        }
    }
}

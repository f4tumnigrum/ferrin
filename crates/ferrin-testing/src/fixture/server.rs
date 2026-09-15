//! The fixture HTTP server (hyper 1.x, HTTP/1.1).

use std::convert::Infallible;
use std::fmt;
use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use bytes::Bytes;
use ferrin_spec::Headers;
use futures_util::stream;
use http::Method;
use http::Request;
use http::Response;
use http::StatusCode;
use http_body_util::BodyExt;
use http_body_util::Full;
use http_body_util::StreamBody;
use http_body_util::combinators::UnsyncBoxBody;
use hyper::body::Frame;
use hyper::body::Incoming;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper_util::rt::TokioIo;
use tokio::net::TcpListener;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use url::Url;

use super::files::Fixture;
use super::files::FixtureBody;

type ResponseBody = UnsyncBoxBody<Bytes, Infallible>;

/// A request the server received.
#[derive(Debug, Clone)]
pub struct ReceivedRequest {
    /// Method.
    pub method: Method,
    /// Path without the query string.
    pub path: String,
    /// Query string, if any.
    pub query: Option<String>,
    /// Headers as received.
    pub headers: Headers,
    /// Body bytes.
    pub body: Bytes,
}

impl ReceivedRequest {
    /// The body as JSON.
    ///
    /// # Errors
    ///
    /// Returns the parse error when the body is not JSON.
    pub fn body_json(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::from_slice(&self.body)
    }

    /// The body as UTF-8 text (lossy).
    #[must_use]
    pub fn body_text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }

    /// A header value as text.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get_str(name)
    }
}

struct Route {
    method: Method,
    path: String,
    fixture: Fixture,
    remaining: Option<usize>,
}

#[derive(Default)]
struct State {
    routes: Mutex<Vec<Route>>,
    received: Mutex<Vec<ReceivedRequest>>,
    chunk_delay: Mutex<Option<Duration>>,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// A local HTTP/1.1 server replaying [`Fixture`]s.
///
/// Routes match on method and path (first mounted wins); unmatched requests
/// get a `404` JSON error. Every request is recorded, matched or not. The
/// server stops when the value is dropped.
pub struct FixtureServer {
    addr: SocketAddr,
    state: Arc<State>,
    shutdown: CancellationToken,
    _tasks: JoinSet<()>,
}

impl fmt::Debug for FixtureServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FixtureServer")
            .field("addr", &self.addr)
            .field("routes", &lock(&self.state.routes).len())
            .field("received", &lock(&self.state.received).len())
            .finish()
    }
}

impl FixtureServer {
    /// Binds `127.0.0.1` on a free port and starts serving.
    ///
    /// # Errors
    ///
    /// Returns the bind error.
    pub async fn start() -> io::Result<Self> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let addr = listener.local_addr()?;
        let state = Arc::new(State::default());
        let shutdown = CancellationToken::new();
        let mut tasks = JoinSet::new();
        tasks.spawn(serve(listener, Arc::clone(&state), shutdown.clone()));
        Ok(Self {
            addr,
            state,
            shutdown,
            _tasks: tasks,
        })
    }

    /// The bound address.
    #[must_use]
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// `http://127.0.0.1:<port>` without a trailing slash.
    #[must_use]
    pub fn uri(&self) -> String {
        format!("http://{}", self.addr)
    }

    /// The base URL.
    ///
    /// # Panics
    ///
    /// Never: the address always forms a valid URL.
    #[must_use]
    pub fn url(&self) -> Url {
        match Url::parse(&format!("{}/", self.uri())) {
            Ok(url) => url,
            Err(_) => unreachable!("a socket address forms a valid URL"),
        }
    }

    /// Mounts `fixture` for every `method` request to `path`.
    pub fn mount(&self, method: Method, path: impl Into<String>, fixture: Fixture) {
        self.mount_route(method, path.into(), fixture, None);
    }

    /// Mounts `fixture` for the next `times` matching requests only.
    pub fn mount_times(
        &self,
        method: Method,
        path: impl Into<String>,
        fixture: Fixture,
        times: usize,
    ) {
        if times != 0 {
            self.mount_route(method, path.into(), fixture, Some(times));
        }
    }

    /// Mounts `fixture` for the next matching request only.
    pub fn mount_once(&self, method: Method, path: impl Into<String>, fixture: Fixture) {
        self.mount_times(method, path, fixture, 1);
    }

    /// Loads `case` from `dir` (see [`Fixture::load`]) and mounts it.
    ///
    /// # Errors
    ///
    /// See [`Fixture::load`].
    pub fn mount_file(
        &self,
        method: Method,
        path: impl Into<String>,
        dir: impl AsRef<Path>,
        case: &str,
    ) -> io::Result<()> {
        let fixture = Fixture::load(dir, case)?;
        self.mount(method, path, fixture);
        Ok(())
    }

    fn mount_route(
        &self,
        method: Method,
        path: String,
        fixture: Fixture,
        remaining: Option<usize>,
    ) {
        lock(&self.state.routes).push(Route {
            method,
            path,
            fixture,
            remaining,
        });
    }

    /// Default delay between events for fixtures that do not set their own.
    pub fn set_chunk_delay(&self, delay: Option<Duration>) {
        *lock(&self.state.chunk_delay) = delay;
    }

    /// Requests received so far, oldest first.
    #[must_use]
    pub fn received(&self) -> Vec<ReceivedRequest> {
        lock(&self.state.received).clone()
    }

    /// Number of requests received so far.
    #[must_use]
    pub fn received_count(&self) -> usize {
        lock(&self.state.received).len()
    }

    /// The most recent request.
    #[must_use]
    pub fn last_received(&self) -> Option<ReceivedRequest> {
        lock(&self.state.received).last().cloned()
    }

    /// Removes all routes and recorded requests.
    pub fn reset(&self) {
        lock(&self.state.routes).clear();
        lock(&self.state.received).clear();
    }
}

impl Drop for FixtureServer {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

async fn serve(listener: TcpListener, state: Arc<State>, shutdown: CancellationToken) {
    let mut connections: JoinSet<()> = JoinSet::new();
    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else {
                    continue;
                };
                let state = Arc::clone(&state);
                let shutdown = shutdown.clone();
                connections.spawn(async move {
                    let io = TokioIo::new(stream);
                    let service = service_fn(move |request| handle(Arc::clone(&state), request));
                    let connection = http1::Builder::new().serve_connection(io, service);
                    tokio::select! {
                        _ = connection => {}
                        () = shutdown.cancelled() => {}
                    }
                });
            }
            Some(_) = connections.join_next() => {}
        }
    }
    connections.abort_all();
}

async fn handle(
    state: Arc<State>,
    request: Request<Incoming>,
) -> Result<Response<ResponseBody>, Infallible> {
    let (parts, body) = request.into_parts();
    let body = match body.collect().await {
        Ok(collected) => collected.to_bytes(),
        Err(error) => {
            return Ok(plain_response(
                StatusCode::BAD_REQUEST,
                "application/json",
                Bytes::from(
                    serde_json::json!({ "error": format!("failed to read request body: {error}") })
                        .to_string(),
                ),
            ));
        }
    };
    let path = parts.uri.path().to_owned();
    let query = parts.uri.query().map(str::to_owned);
    lock(&state.received).push(ReceivedRequest {
        method: parts.method.clone(),
        path: path.clone(),
        query,
        headers: Headers::from_map(parts.headers),
        body,
    });
    let path_and_query = parts
        .uri
        .path_and_query()
        .map_or_else(|| path.clone(), |value| value.as_str().to_owned());
    let matched = {
        let mut routes = lock(&state.routes);
        let position = routes.iter().position(|route| {
            route.method == parts.method && (route.path == path || route.path == path_and_query)
        });
        position.map(|index| {
            let consumed = match &mut routes[index].remaining {
                Some(remaining) => {
                    *remaining = remaining.saturating_sub(1);
                    *remaining == 0
                }
                None => false,
            };
            if consumed {
                routes.remove(index).fixture
            } else {
                routes[index].fixture.clone()
            }
        })
    };
    let Some(fixture) = matched else {
        return Ok(plain_response(
            StatusCode::NOT_FOUND,
            "application/json",
            Bytes::from(
                serde_json::json!({
                    "error": format!("no fixture mounted for {} {path}", parts.method)
                })
                .to_string(),
            ),
        ));
    };
    let default_delay = *lock(&state.chunk_delay);
    Ok(fixture_response(fixture, default_delay))
}

fn plain_response(status: StatusCode, content_type: &str, body: Bytes) -> Response<ResponseBody> {
    let mut response = Response::new(Full::new(body).boxed_unsync());
    *response.status_mut() = status;
    if let Ok(value) = http::HeaderValue::from_str(content_type) {
        response
            .headers_mut()
            .insert(http::header::CONTENT_TYPE, value);
    }
    response
}

fn fixture_response(fixture: Fixture, default_delay: Option<Duration>) -> Response<ResponseBody> {
    let Fixture {
        status,
        headers,
        body,
    } = fixture;
    let body: ResponseBody = match body {
        FixtureBody::Complete(bytes) => Full::new(bytes).boxed_unsync(),
        FixtureBody::Events {
            events,
            initial_delay,
            chunk_delay,
            hold_open,
        } => {
            let delay = chunk_delay.or(default_delay);
            let frames = stream::unfold(
                (events.into_iter(), 0usize),
                move |(mut events, index)| async move {
                    let Some(event) = events.next() else {
                        if hold_open {
                            // The connection task is aborted when the client
                            // disconnects or the server is dropped.
                            std::future::pending::<()>().await;
                        }
                        return None;
                    };
                    let wait = if index == 0 { initial_delay } else { delay };
                    if let Some(wait) = wait {
                        tokio::time::sleep(wait).await;
                    }
                    let frame = Frame::data(Bytes::from(format!("{event}\n\n")));
                    Some((Ok::<_, Infallible>(frame), (events, index + 1)))
                },
            );
            StreamBody::new(frames).boxed_unsync()
        }
    };
    let mut response = Response::new(body);
    *response.status_mut() = status;
    *response.headers_mut() = headers.into_map();
    response
}

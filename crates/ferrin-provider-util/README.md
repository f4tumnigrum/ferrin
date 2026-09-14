# ferrin-provider-util

Shared infrastructure for Ferrin provider adapters.

Part of the [Ferrin](../../README.md) workspace. Design:
`docs/01-architecture/14-http-and-security.md`, `docs/01-architecture/02-crates.md`.

## Contents

- `http`: the `HttpTransport` trait, `HttpRequest`/`HttpResponse`/`RequestBody`
  (raw bytes or hand-encoded multipart), `TransportError`, the reqwest-based
  `ReqwestTransport` (feature `reqwest`, on by default: rustls, HTTP/2, no
  automatic redirects, no default timeout, DNS pinning per request), the
  request helpers `post_json`/`post_form`/`post_bytes`/`get`/`delete`/`send`,
  and response handlers (JSON, JSON error, text, binary, binary stream,
  server-sent events, JSON Lines, status code).
- `sse`: incremental WHATWG server-sent-events decoder with per-event size
  limit and receive timestamps.
- `secure_url`: `UrlPolicy` (scheme allow-list, private-network rejection,
  trusted and credentialed origins, redirect and body limits), `validate_url`
  with DNS resolution and address pinning, `fetch`/`fetch_with_headers` with
  manual validated redirects.
- `settings`: `load_api_key`, `load_setting`, `load_optional_setting` (the
  only environment lookups in the workspace).
- `ids`: `IdGenerator`, `PrefixedIdGenerator`, `generate_id`.
- `media_type`: magic-number detection for images, PDF, audio and video;
  extension mapping; partial media type resolution.
- `reasoning`: reasoning level to provider effort/budget mapping with
  warnings.
- `tool_name_mapping`, `streaming_tool_call`, `provider_options`,
  `provider_reference`, `response_metadata`, `batch`, `base_url`, `headers`,
  `retry` (status classification, `Retry-After` parsing).

## Example

```rust,no_run
use ferrin_provider_util::ResponseHandlers;
use ferrin_provider_util::http::json_error_response_handler;
use ferrin_provider_util::http::json_response_handler;
use ferrin_provider_util::http::post_json;
use ferrin_spec::Headers;
use tokio_util::sync::CancellationToken;

#[derive(serde::Deserialize)]
struct Reply { id: String }

#[derive(serde::Deserialize)]
struct ApiError { message: String }

async fn call() -> Result<Reply, ferrin_spec::ProviderError> {
    let transport = ferrin_provider_util::default_transport()
        .map_err(|error| ferrin_spec::ProviderError::message(error.to_string()))?;
    let handlers = ResponseHandlers::new(
        json_response_handler::<Reply>(),
        json_error_response_handler::<ApiError>(|error| error.message.clone()),
    );
    let response = post_json(
        transport.as_ref(),
        "https://api.example.com/v1/things".parse().map_err(|_| ferrin_spec::ProviderError::message("bad url"))?,
        Headers::new().with("authorization", "Bearer ..."),
        &serde_json::json!({ "name": "x" }),
        &handlers,
        CancellationToken::new(),
    )
    .await?;
    Ok(response.value)
}
```

## Features

| Feature | Default | Effect |
|---|---|---|
| `reqwest` | on | `ReqwestTransport` and `default_transport()` |
| `platform-verifier` | off | Re-exports `rustls_platform_verifier` for platform initialisation hooks |

## License

MIT OR Apache-2.0.

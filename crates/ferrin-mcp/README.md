# ferrin-mcp

MCP (Model Context Protocol) client for Ferrin: JSON-RPC 2.0 messages, the
Streamable HTTP, legacy SSE and stdio transports, protocol negotiation for the
2026-07-28 and 2025-11-25 protocol generations, tools/resources/prompts/
completion methods, bridging of MCP tools into a `ferrin_tool::ToolSet`, MCP
Apps helpers and (feature `oauth`) the OAuth 2.1 authorization flow with PKCE.

Part of the [Ferrin](https://github.com/f4tumnigrum/ferrin) workspace. Design:
`docs/01-architecture/15-mcp.md`, ADR 0015.

## Example

```rust
use ferrin_mcp::McpClient;
use ferrin_mcp::McpClientConfig;
use ferrin_mcp::ToolsOptions;
use ferrin_mcp::transport::HttpTransportConfig;
use ferrin_mcp::transport::TransportConfig;
use ferrin_spec::Headers;

async fn mcp_tools() -> Result<ferrin_tool::ToolSet, ferrin_mcp::McpError> {
    let url = url::Url::parse("https://mcp.example.com/mcp")
        .map_err(|error| ferrin_mcp::McpError::invalid_argument(error.to_string()))?;
    let transport = HttpTransportConfig::new(url)
        .headers(Headers::new().with("x-tenant", "acme"));
    let client = McpClient::connect(
        McpClientConfig::new(TransportConfig::Http(transport)).name("my-app"),
    )
    .await?;
    let tools = client.tools(ToolsOptions::default()).await?;
    client.close().await?;
    Ok(tools)
}
```

The client probes `server/discover` first and runs the 2026-07-28 protocol
without sessions when the server supports it; otherwise it falls back to
`initialize` with sessions, the GET inbound stream and `DELETE` termination.
Endpoints must satisfy the secure URL policy of `ferrin-provider-util`
(HTTPS, public networks) unless the transport config opts in with
`.url_policy(UrlPolicy::new().allow_http().allow_private_networks())`.

## Features

| Feature | Default | Effect |
|---|---|---|
| `stdio` | on | `StdioTransport`/`StdioConfig` (child process over stdin/stdout); enables `tokio/process` and `tokio/io-util` |
| `oauth` | on | `oauth` module, `HttpTransportConfig::auth_provider`, `SseTransportConfig::auth_provider` |

## Testing

`cargo nextest run -p ferrin-mcp --all-features` runs the transport tests
against the fixture server of `ferrin-testing`, the client tests against an
in-process mock transport, and the stdio tests against
`tests/fixtures/stdio/echo_server.py`, which requires `python3` on `PATH`.

## License

Apache-2.0. See [LICENSE](LICENSE) and [NOTICE](NOTICE). Portions of this crate are derived from the Vercel AI SDK (Apache-2.0); the crate and module documentation carry the attribution.

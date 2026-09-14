//! MCP (Model Context Protocol) client for Ferrin.
//!
//! The crate implements JSON-RPC framing, the Streamable HTTP, legacy SSE
//! and stdio transports, protocol negotiation for the 2024-11-05 to
//! 2026-07-28 protocol versions, tool bridging into [`ferrin_tool::ToolSet`],
//! MCP Apps helpers and (with the `oauth` feature) the OAuth 2.1
//! authorization flow.

pub mod apps;
pub mod client;
pub mod error;
#[cfg(feature = "oauth")]
pub mod oauth;
pub mod protocol;
pub mod tools;
pub mod transport;

pub use client::ElicitationHandler;
pub use client::McpClient;
pub use client::McpClientConfig;
pub use client::NotificationHook;
pub use client::RequestOptions;
pub use client::UncaughtErrorHook;
pub use client::elicitation_handler;
pub use error::BoxError;
pub use error::McpError;
pub use error::TransportFailure;
pub use tools::McpToolExecutor;
pub use tools::ToolSchemaPair;
pub use tools::ToolSchemas;
pub use tools::ToolsOptions;
pub use tools::mcp_to_model_output;

//! Connects to an MCP server over stdio, exposes its tools to the model and
//! runs one tool-calling conversation.
//!
//! ```text
//! OPENAI_API_KEY=... cargo run -p example-mcp
//! ```
//!
//! By default the example launches `npx -y @modelcontextprotocol/server-everything`
//! (requires Node.js). `MCP_SERVER_COMMAND` and `MCP_SERVER_ARGS`
//! (whitespace-separated) select another server. `OPENAI_BASE_URL`,
//! `OPENAI_MODEL` (default `gpt-5`) and `OPENAI_PROVIDER_OPTIONS` (JSON)
//! configure the model.

#![allow(clippy::print_stdout)]

use ferrin::mcp::McpClient;
use ferrin::mcp::McpClientConfig;
use ferrin::mcp::ToolsOptions;
use ferrin::mcp::transport::StdioConfig;
use ferrin::mcp::transport::TransportConfig;
use ferrin::openai::OpenAiSettings;
use ferrin::openai::create_openai;
use ferrin::prelude::*;
use ferrin::provider_util::settings::env_var;

/// Extra provider options from `OPENAI_PROVIDER_OPTIONS`, a JSON object keyed
/// by provider name. Example for endpoints that do not store response items:
/// `{"openai":{"store":false}}`.
fn provider_options() -> anyhow::Result<Option<ProviderOptions>> {
    env_var("OPENAI_PROVIDER_OPTIONS")
        .map(|text| Ok(ferrin::serde_json::from_str(&text)?))
        .transpose()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let command = env_var("MCP_SERVER_COMMAND").unwrap_or_else(|| "npx".to_owned());
    let args: Vec<String> = match env_var("MCP_SERVER_ARGS") {
        Some(args) => args.split_whitespace().map(str::to_owned).collect(),
        None => vec![
            "-y".to_owned(),
            "@modelcontextprotocol/server-everything".to_owned(),
        ],
    };
    let transport = TransportConfig::Stdio(StdioConfig::new(command).args(args));
    let client = McpClient::connect(McpClientConfig::new(transport).name("example-mcp")).await?;
    if let Some(server) = client.server_info() {
        println!("connected to {} {}", server.name, server.version);
    }

    // Every server tool becomes a Ferrin tool; input schemas come from the
    // server's `tools/list` result.
    let tools = client.tools(ToolsOptions::default()).await?;
    println!("{} tools:", tools.len());
    for name in tools.names() {
        println!("- {name}");
    }

    let openai = create_openai(OpenAiSettings::default())?;
    let model_id = env_var("OPENAI_MODEL").unwrap_or_else(|| "gpt-5".to_owned());
    let mut call = generate_text(openai.responses(&model_id))
        .prompt("Use the `get-sum` tool to compute 21 + 21 and report the result in one sentence.")
        .tools(tools)
        .stop_when(step_count(4));
    if let Some(options) = provider_options()? {
        call = call.provider_options(options);
    }
    let result = call.await?;

    println!();
    for step in &result.steps {
        for call in step.tool_calls() {
            println!("-> {}({})", call.tool_name, call.input);
        }
    }
    println!("{}", result.text());

    client.close().await?;
    Ok(())
}

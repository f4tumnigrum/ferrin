//! Human-in-the-loop tool approval.
//!
//! The `delete_file` tool always requires approval. The first call stops with
//! a pending approval request; the program asks on the terminal, appends the
//! approval response to the conversation and continues the call.
//!
//! ```text
//! OPENAI_API_KEY=... cargo run -p example-tool-approval
//! ```
//!
//! `OPENAI_BASE_URL` and `OPENAI_MODEL` (default `gpt-5`) select the endpoint
//! and model; `OPENAI_PROVIDER_OPTIONS` passes provider options as JSON.

#![allow(clippy::print_stdout)]

use ferrin::openai::OpenAiSettings;
use ferrin::openai::create_openai;
use ferrin::prelude::*;
use ferrin::provider_util::settings::env_var;
use tokio::io::AsyncBufReadExt;
use tokio::io::BufReader;

#[derive(Deserialize, JsonSchema)]
#[serde(crate = "ferrin::serde")]
#[schemars(crate = "ferrin::schemars")]
struct DeleteFile {
    /// Path of the file to delete.
    path: String,
}

/// A tool set with one tool that always waits for a human.
fn tools() -> anyhow::Result<ToolSet> {
    let delete_file = Tool::function::<DeleteFile>()
        .description("Deletes a file from the workspace.")
        .needs_approval(NeedsApproval::Always)
        .execute(|input: DeleteFile, _ctx: ToolContext| async move {
            // A real tool would remove the file here.
            Ok::<_, ToolError>(json!({ "deleted": input.path }))
        })
        .build();
    Ok(ToolSet::new().insert("delete_file", delete_file)?)
}

/// Extra provider options from `OPENAI_PROVIDER_OPTIONS`, a JSON object keyed
/// by provider name. Example for endpoints that do not store response items:
/// `{"openai":{"store":false}}`.
fn provider_options() -> anyhow::Result<Option<ProviderOptions>> {
    env_var("OPENAI_PROVIDER_OPTIONS")
        .map(|text| Ok(ferrin::serde_json::from_str(&text)?))
        .transpose()
}

async fn ask(question: &str) -> anyhow::Result<bool> {
    println!("{question} [y/N] ");
    let mut line = String::new();
    BufReader::new(tokio::io::stdin())
        .read_line(&mut line)
        .await?;
    Ok(matches!(line.trim(), "y" | "Y" | "yes"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let openai = create_openai(OpenAiSettings::default())?;
    let model_id = env_var("OPENAI_MODEL").unwrap_or_else(|| "gpt-5".to_owned());

    let mut messages = vec![Message::user(
        "Delete the file build/cache.bin, then confirm what you did.",
    )];
    let mut first = generate_text(openai.responses(&model_id))
        .messages(messages.clone())
        .tools(tools()?);
    if let Some(options) = provider_options()? {
        first = first.provider_options(options);
    }
    let first = first.await?;
    messages.extend(first.response_messages());

    let requests: Vec<_> = first.last_step().tool_approval_requests().collect();
    if requests.is_empty() {
        println!("{}", first.text());
        return Ok(());
    }
    for request in requests {
        let approved = ask(&format!(
            "Allow `{}` with input {}?",
            request.tool_call.tool_name, request.tool_call.input
        ))
        .await?;
        let response = if approved {
            ToolApprovalResponse::approved(request.approval_id.clone())
        } else {
            ToolApprovalResponse::denied(request.approval_id.clone())
                .with_reason("declined by the operator")
        };
        messages.push_approval_response(response);
    }

    // The second call executes the approved tool calls (or reports the
    // denial to the model) and lets the model finish its answer.
    let mut second = generate_text(openai.responses(&model_id))
        .messages(messages)
        .tools(tools()?)
        .stop_when(step_count(3));
    if let Some(options) = provider_options()? {
        second = second.provider_options(options);
    }
    let second = second.await?;
    println!("{}", second.text());
    Ok(())
}

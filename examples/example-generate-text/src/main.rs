//! Single-step text generation with the OpenAI Responses API.
//!
//! ```text
//! OPENAI_API_KEY=... cargo run -p example-generate-text
//! ```
//!
//! `OPENAI_MODEL` overrides the model id (default `gpt-5`).

#![allow(clippy::print_stdout)]

use ferrin::openai::OpenAiSettings;
use ferrin::openai::create_openai;
use ferrin::prelude::*;
use ferrin::provider_util::settings::env_var;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let openai = create_openai(OpenAiSettings::default())?;
    let model_id = env_var("OPENAI_MODEL").unwrap_or_else(|| "gpt-5".to_owned());

    let result = generate_text(openai.responses(&model_id))
        .system("You are a concise assistant.")
        .prompt("Explain backpressure in streaming systems in two sentences.")
        .max_output_tokens(300)
        .await?;

    println!("{}", result.text());
    println!();
    println!("finish reason: {}", result.finish_reason());
    let usage = result.usage();
    println!(
        "usage: {} input tokens, {} output tokens",
        usage.input.total.unwrap_or(0),
        usage.output.total.unwrap_or(0)
    );
    for warning in result.warnings() {
        println!("warning: {warning:?}");
    }
    Ok(())
}

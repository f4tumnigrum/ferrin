/// Uppercase the input.
#[ferrin::tool]
async fn upper(input: &str) -> Result<String, ferrin::tool::ToolError> {
    Ok(input.to_uppercase())
}

fn main() {}

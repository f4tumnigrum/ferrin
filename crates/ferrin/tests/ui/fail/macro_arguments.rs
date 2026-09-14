/// Echo.
#[ferrin::tool(name = "echo")]
async fn echo(input: String) -> Result<String, ferrin::tool::ToolError> {
    Ok(input)
}

fn main() {}

/// Join the inputs.
#[ferrin::tool]
async fn join(input: Vec<&str>) -> Result<String, ferrin::tool::ToolError> {
    Ok(input.concat())
}

fn main() {}

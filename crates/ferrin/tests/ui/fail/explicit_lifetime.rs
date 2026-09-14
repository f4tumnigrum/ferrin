/// Uppercase the input.
#[ferrin::tool]
async fn upper<'a>(input: std::borrow::Cow<'a, str>) -> Result<String, ferrin::tool::ToolError> {
    Ok(input.to_uppercase())
}

fn main() {}

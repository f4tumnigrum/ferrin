/// Echo the input.
#[ferrin::tool]
async fn echo<T: ferrin::serde::Serialize>(input: T) -> Result<T, ferrin::tool::ToolError> {
    Ok(input)
}

fn main() {}

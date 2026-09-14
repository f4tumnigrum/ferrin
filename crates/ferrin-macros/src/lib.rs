//! Ferrin procedural macros.
//!
//! Provides `#[ferrin::tool]`, which turns a function into a tool
//! constructor. Use it through the `ferrin` facade (the generated code refers
//! to `::ferrin::tool::*`); this crate has no stable API of its own.
//!
//! Design: `docs/01-architecture/06-tool-system.md` §1.1.

use proc_macro::TokenStream;

mod tool;

/// Turns a function into a constructor of a [`Tool`](../ferrin/tool/struct.Tool.html).
///
/// The function takes one owned input parameter (a type implementing
/// `serde::Deserialize` and `schemars::JsonSchema`, which becomes the input
/// schema) and optionally a second `ToolContext` parameter, and returns
/// `Result<O, ToolError>` where `O: serde::Serialize`. It may be `async`.
/// The doc comment becomes the tool description. The macro replaces the
/// function with `fn name() -> Tool`; reference parameters and explicit
/// lifetimes are rejected at expansion time because tool executors must be
/// `'static`.
///
/// ```rust,ignore
/// /// Get the current weather for a city.
/// #[ferrin::tool]
/// async fn get_weather(input: GetWeather) -> Result<Weather, ToolError> {
///     Ok(Weather { temperature_c: 21.5 })
/// }
///
/// let tools = ToolSet::new().insert("get_weather", get_weather())?;
/// ```
#[proc_macro_attribute]
pub fn tool(attr: TokenStream, item: TokenStream) -> TokenStream {
    tool::expand(attr.into(), item.into())
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

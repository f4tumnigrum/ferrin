use std::sync::Arc;

use ferrin_core::LanguageModelMiddleware;
use ferrin_core::generate_text;
use ferrin_core::middleware::GenerateNext;
use ferrin_core::middleware::MiddlewareContext;
use ferrin_core::wrap_language_model;
use ferrin_spec::BoxFuture;
use ferrin_spec::CallOptions;
use ferrin_spec::GenerateResult;
use ferrin_spec::ToolChoice;
use ferrin_spec::error::ProviderError;
use pretty_assertions::assert_eq;

use crate::suite::common::text_model;
use crate::suite::common::weather_tools;

struct ClearInContinuation;

impl LanguageModelMiddleware for ClearInContinuation {
    fn wrap_generate<'a>(
        &'a self,
        mut options: CallOptions,
        next: GenerateNext<'a>,
        _ctx: MiddlewareContext<'a>,
    ) -> BoxFuture<'a, Result<GenerateResult, ProviderError>> {
        options.tools.clear();
        options.tool_choice = None;
        next(options)
    }
}

#[tokio::test]
async fn continuation_changes_update_the_execution_contract() {
    let model = wrap_language_model(
        text_model("ok"),
        [Arc::new(ClearInContinuation) as Arc<dyn LanguageModelMiddleware>],
    );
    let result = generate_text(model)
        .prompt("hi")
        .tools(weather_tools())
        .tool_choice(ToolChoice::Required)
        .await
        .unwrap();
    assert_eq!(result.text(), "ok");
}

//! [`ToolBuilder`] and the closure adapters behind `execute`.

use std::marker::PhantomData;
use std::sync::Arc;

use ferrin_schema::JsonSchema;
use ferrin_schema::Schema;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderOptions;
use ferrin_spec::language_model::prompt::ToolResultOutput;
use futures_core::Stream;
use futures_util::StreamExt;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::callers::ToolCallerDefinition;
use crate::error::ToolError;
use crate::execute::ToolContext;
use crate::execute::ToolExecute;
use crate::execute::ToolOutput;
use crate::execute::ToolOutputStream;
use crate::tool::Description;
use crate::tool::DescriptionContext;
use crate::tool::ModelOutputArgs;
use crate::tool::NeedsApproval;
use crate::tool::Tool;
use crate::tool::ToolHooks;
use crate::tool::ToolKind;

/// Builds a [`Tool`]. `I` is the input type handed to the execute closure.
pub struct ToolBuilder<I> {
    tool: Tool,
    _input: PhantomData<fn() -> I>,
}

impl<I> std::fmt::Debug for ToolBuilder<I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolBuilder")
            .field("tool", &self.tool)
            .finish()
    }
}

fn base(kind: ToolKind, input_schema: Schema<JsonValue>) -> Tool {
    Tool {
        kind,
        description: None,
        title: None,
        input_schema,
        output_schema: None,
        context_schema: None,
        execute: None,
        needs_approval: NeedsApproval::Never,
        strict: None,
        input_examples: Vec::new(),
        metadata: None,
        provider_options: None,
        hooks: ToolHooks::default(),
        to_model_output: None,
        caller_definition: None,
    }
}

impl Tool {
    /// A function tool whose input schema is derived from `I`
    /// (draft-07, `additionalProperties: false` on objects).
    #[must_use]
    pub fn function<I: DeserializeOwned + JsonSchema + 'static>() -> ToolBuilder<I> {
        ToolBuilder {
            tool: base(ToolKind::Function, Schema::<I>::derived().erased()),
            _input: PhantomData,
        }
    }

    /// A function tool with an explicit JSON schema; the execute closure
    /// receives the raw JSON input.
    #[must_use]
    pub fn function_with_schema(input_schema: Schema<JsonValue>) -> ToolBuilder<JsonValue> {
        ToolBuilder {
            tool: base(ToolKind::Function, input_schema),
            _input: PhantomData,
        }
    }

    /// A dynamic tool (runtime-defined schema, untyped input and output).
    #[must_use]
    pub fn dynamic(input_schema: Schema<JsonValue>) -> ToolBuilder<JsonValue> {
        ToolBuilder {
            tool: base(ToolKind::Dynamic, input_schema),
            _input: PhantomData,
        }
    }

    /// A provider-defined tool executed by the application. The input schema
    /// defaults to "any"; provider crates set the real one.
    #[must_use]
    pub fn provider_defined(id: impl Into<String>, args: JsonObject) -> ToolBuilder<JsonValue> {
        ToolBuilder {
            tool: base(
                ToolKind::ProviderDefined {
                    id: id.into(),
                    args,
                },
                Schema::any(),
            ),
            _input: PhantomData,
        }
    }

    /// A provider-executed tool.
    #[must_use]
    pub fn provider_executed(id: impl Into<String>, args: JsonObject) -> ToolBuilder<JsonValue> {
        ToolBuilder {
            tool: base(
                ToolKind::ProviderExecuted {
                    id: id.into(),
                    args,
                    supports_deferred_results: false,
                },
                Schema::any(),
            ),
            _input: PhantomData,
        }
    }
}

impl<I> ToolBuilder<I> {
    /// Fixed description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.tool.description = Some(Description::Static(description.into()));
        self
    }

    /// Description computed per call.
    #[must_use]
    pub fn description_fn<F, Fut>(mut self, function: F) -> Self
    where
        F: Fn(DescriptionContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = String> + Send + 'static,
    {
        self.tool.description = Some(Description::Dynamic(Arc::new(move |ctx| {
            Box::pin(function(ctx))
        })));
        self
    }

    /// Title.
    #[must_use]
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.tool.title = Some(title.into());
        self
    }

    /// Replaces the input schema (keeps the closure input type).
    #[must_use]
    pub fn input_schema(mut self, schema: Schema<JsonValue>) -> Self {
        self.tool.input_schema = schema;
        self
    }

    /// Output schema.
    #[must_use]
    pub fn output_schema(mut self, schema: Schema<JsonValue>) -> Self {
        self.tool.output_schema = Some(schema);
        self
    }

    /// Context schema.
    #[must_use]
    pub fn context_schema(mut self, schema: Schema<JsonValue>) -> Self {
        self.tool.context_schema = Some(schema);
        self
    }

    /// Approval declaration.
    #[must_use]
    pub fn needs_approval(mut self, needs_approval: NeedsApproval) -> Self {
        self.tool.needs_approval = needs_approval;
        self
    }

    /// Approval decided per call from the validated input.
    #[must_use]
    pub fn needs_approval_if<F, Fut>(mut self, function: F) -> Self
    where
        F: Fn(JsonValue, ToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        self.tool.needs_approval =
            NeedsApproval::Dynamic(Arc::new(move |input, ctx| Box::pin(function(input, ctx))));
        self
    }

    /// Strict-mode request.
    #[must_use]
    pub fn strict(mut self, strict: bool) -> Self {
        self.tool.strict = Some(strict);
        self
    }

    /// Adds an example input.
    #[must_use]
    pub fn input_example(mut self, example: JsonObject) -> Self {
        self.tool.input_examples.push(example);
        self
    }

    /// Adds example inputs.
    #[must_use]
    pub fn input_examples(mut self, examples: impl IntoIterator<Item = JsonObject>) -> Self {
        self.tool.input_examples.extend(examples);
        self
    }

    /// Metadata propagated to tool calls.
    #[must_use]
    pub fn metadata(mut self, metadata: JsonObject) -> Self {
        self.tool.metadata = Some(metadata);
        self
    }

    /// Provider options sent with the definition.
    #[must_use]
    pub fn provider_options(mut self, options: ProviderOptions) -> Self {
        self.tool.provider_options = Some(options);
        self
    }

    /// Hook called when the model starts producing input.
    #[must_use]
    pub fn on_input_start<F, Fut>(mut self, function: F) -> Self
    where
        F: Fn(ToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.tool.hooks.on_input_start = Some(Arc::new(move |ctx| Box::pin(function(ctx))));
        self
    }

    /// Hook called for each input delta.
    #[must_use]
    pub fn on_input_delta<F, Fut>(mut self, function: F) -> Self
    where
        F: Fn(String, ToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.tool.hooks.on_input_delta =
            Some(Arc::new(move |delta, ctx| Box::pin(function(delta, ctx))));
        self
    }

    /// Hook called once the input is complete and valid.
    #[must_use]
    pub fn on_input_available<F, Fut>(mut self, function: F) -> Self
    where
        F: Fn(JsonValue, ToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = ()> + Send + 'static,
    {
        self.tool.hooks.on_input_available =
            Some(Arc::new(move |input, ctx| Box::pin(function(input, ctx))));
        self
    }

    /// Custom conversion of the output into what the model receives.
    #[must_use]
    pub fn to_model_output<F>(mut self, function: F) -> Self
    where
        F: Fn(ModelOutputArgs<'_>) -> ToolResultOutput + Send + Sync + 'static,
    {
        self.tool.to_model_output = Some(Arc::new(function));
        self
    }

    /// Declares how this tool calls other tools.
    #[must_use]
    pub fn caller(mut self, definition: ToolCallerDefinition) -> Self {
        self.tool.caller_definition = Some(definition);
        self
    }

    /// Marks a provider-executed tool as supporting deferred results. Ignored
    /// for other kinds.
    #[must_use]
    pub fn supports_deferred_results(mut self, supported: bool) -> Self {
        if let ToolKind::ProviderExecuted {
            supports_deferred_results,
            ..
        } = &mut self.tool.kind
        {
            *supports_deferred_results = supported;
        }
        self
    }

    /// Uses a custom executor.
    #[must_use]
    pub fn execute_with(mut self, executor: Arc<dyn ToolExecute>) -> Self {
        self.tool.execute = Some(executor);
        self
    }

    /// Finishes the tool.
    #[must_use]
    pub fn build(self) -> Tool {
        self.tool
    }
}

impl<I: DeserializeOwned + Send + 'static> ToolBuilder<I> {
    /// Executes with an async closure returning a single result.
    #[must_use]
    pub fn execute<F, Fut, O>(mut self, function: F) -> Self
    where
        F: Fn(I, ToolContext) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<O, ToolError>> + Send + 'static,
        O: Serialize + 'static,
    {
        self.tool.execute = Some(Arc::new(FnExecute {
            function,
            _marker: PhantomData::<fn() -> (I, O)>,
        }));
        self
    }

    /// Executes with a closure returning a stream; every item becomes a
    /// preliminary output and the last one is repeated as the final output.
    #[must_use]
    pub fn execute_stream<F, S, O>(mut self, function: F) -> Self
    where
        F: Fn(I, ToolContext) -> S + Send + Sync + 'static,
        S: Stream<Item = Result<O, ToolError>> + Send + 'static,
        O: Serialize + 'static,
    {
        self.tool.execute = Some(Arc::new(StreamExecute {
            function,
            _marker: PhantomData::<fn() -> (I, O)>,
        }));
        self
    }
}

fn decode<I: DeserializeOwned>(input: JsonValue) -> Result<I, ToolError> {
    serde_json::from_value(input).map_err(|error| {
        ToolError::message(format!("invalid tool input: {error}")).with_cause(error)
    })
}

fn encode<O: Serialize>(output: &O) -> Result<JsonValue, ToolError> {
    serde_json::to_value(output).map_err(|error| {
        ToolError::message(format!("tool output is not serializable: {error}")).with_cause(error)
    })
}

struct FnExecute<I, O, F> {
    function: F,
    _marker: PhantomData<fn() -> (I, O)>,
}

impl<I, O, F, Fut> ToolExecute for FnExecute<I, O, F>
where
    I: DeserializeOwned + Send + 'static,
    O: Serialize + 'static,
    F: Fn(I, ToolContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<O, ToolError>> + Send + 'static,
{
    fn execute(&self, input: JsonValue, ctx: ToolContext) -> ToolOutputStream {
        let future = decode::<I>(input).map(|input| (self.function)(input, ctx));
        Box::pin(futures_util::stream::once(async move {
            let output = future?.await?;
            encode(&output).map(ToolOutput::Final)
        }))
    }
}

struct StreamExecute<I, O, F> {
    function: F,
    _marker: PhantomData<fn() -> (I, O)>,
}

struct StreamState<S> {
    inner: S,
    last: Option<JsonValue>,
    done: bool,
}

impl<I, O, F, S> ToolExecute for StreamExecute<I, O, F>
where
    I: DeserializeOwned + Send + 'static,
    O: Serialize + 'static,
    F: Fn(I, ToolContext) -> S + Send + Sync + 'static,
    S: Stream<Item = Result<O, ToolError>> + Send + 'static,
{
    fn execute(&self, input: JsonValue, ctx: ToolContext) -> ToolOutputStream {
        let inner = match decode::<I>(input) {
            Ok(input) => (self.function)(input, ctx),
            Err(error) => {
                return Box::pin(futures_util::stream::once(std::future::ready(Err(error))));
            }
        };
        let state = StreamState {
            inner: Box::pin(inner),
            last: None,
            done: false,
        };
        Box::pin(futures_util::stream::unfold(
            state,
            |mut state| async move {
                if state.done {
                    return None;
                }
                match state.inner.next().await {
                    Some(Ok(output)) => match encode(&output) {
                        Ok(value) => {
                            state.last = Some(value.clone());
                            Some((Ok(ToolOutput::Preliminary(value)), state))
                        }
                        Err(error) => {
                            state.done = true;
                            Some((Err(error), state))
                        }
                    },
                    Some(Err(error)) => {
                        state.done = true;
                        Some((Err(error), state))
                    }
                    None => {
                        state.done = true;
                        let value = state.last.take().unwrap_or(JsonValue::Null);
                        Some((Ok(ToolOutput::Final(value)), state))
                    }
                }
            },
        ))
    }
}

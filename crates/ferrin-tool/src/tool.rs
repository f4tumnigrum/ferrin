//! The [`Tool`] type and its declarative parts.

use std::fmt;
use std::sync::Arc;

use ferrin_schema::Schema;
use ferrin_spec::BoxFuture;
use ferrin_spec::JsonObject;
use ferrin_spec::JsonValue;
use ferrin_spec::ProviderOptions;
use ferrin_spec::ToolCallId;
use ferrin_spec::ToolDefinition;
use ferrin_spec::ToolName;
use ferrin_spec::error::TypeValidationContext;
use ferrin_spec::error::TypeValidationError;
use ferrin_spec::language_model::prompt::ToolResultOutput;

use crate::callers::ToolCallerDefinition;
use crate::execute::ToolContext;
use crate::execute::ToolExecute;
use crate::execute::ToolOutputStream;

/// Who defines and who executes a tool.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ToolKind {
    /// Application-defined schema, executed by the application (with an
    /// execute function) or by the caller (without one).
    Function,
    /// Defined at runtime (MCP and similar); input and output are untyped.
    Dynamic,
    /// Schema defined by the provider, executed by the application.
    ProviderDefined {
        /// Provider tool id in `<provider>.<tool>` form.
        id: String,
        /// Provider-specific configuration.
        args: JsonObject,
    },
    /// Defined and executed by the provider.
    ProviderExecuted {
        /// Provider tool id in `<provider>.<tool>` form.
        id: String,
        /// Provider-specific configuration.
        args: JsonObject,
        /// The result may arrive in a later turn than the call.
        supports_deferred_results: bool,
    },
}

impl ToolKind {
    /// Returns `true` for provider-defined and provider-executed tools.
    #[must_use]
    pub fn is_provider(&self) -> bool {
        matches!(
            self,
            Self::ProviderDefined { .. } | Self::ProviderExecuted { .. }
        )
    }

    /// Returns `true` when the provider executes the tool.
    #[must_use]
    pub fn is_provider_executed(&self) -> bool {
        matches!(self, Self::ProviderExecuted { .. })
    }

    /// Returns `true` for dynamic tools.
    #[must_use]
    pub fn is_dynamic(&self) -> bool {
        matches!(self, Self::Dynamic)
    }

    /// The provider tool id, for provider tools.
    #[must_use]
    pub fn provider_id(&self) -> Option<&str> {
        match self {
            Self::ProviderDefined { id, .. } | Self::ProviderExecuted { id, .. } => Some(id),
            _ => None,
        }
    }
}

/// Inputs available when resolving a dynamic description.
#[derive(Clone, Default)]
pub struct DescriptionContext {
    /// The validated tool context for this tool, if any.
    pub tool_context: Option<JsonValue>,
    /// The sandbox of the call, if any.
    #[cfg(feature = "sandbox")]
    pub sandbox: Option<Arc<dyn crate::sandbox::Sandbox>>,
}

impl DescriptionContext {
    /// Creates a context carrying `tool_context`.
    #[must_use]
    pub fn with_tool_context(tool_context: JsonValue) -> Self {
        Self {
            tool_context: Some(tool_context),
            #[cfg(feature = "sandbox")]
            sandbox: None,
        }
    }
}

impl fmt::Debug for DescriptionContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("DescriptionContext");
        debug.field("tool_context", &self.tool_context);
        #[cfg(feature = "sandbox")]
        debug.field(
            "sandbox",
            &self.sandbox.as_ref().map(|sandbox| sandbox.description()),
        );
        debug.finish()
    }
}

/// Produces a description per call.
pub type DescriptionFn =
    Arc<dyn Fn(DescriptionContext) -> BoxFuture<'static, String> + Send + Sync>;

/// Tool description sent to the model.
#[derive(Clone)]
#[non_exhaustive]
pub enum Description {
    /// A fixed string.
    Static(String),
    /// Computed from the call context.
    Dynamic(DescriptionFn),
}

impl fmt::Debug for Description {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Static(text) => f.debug_tuple("Static").field(text).finish(),
            Self::Dynamic(_) => f.write_str("Dynamic(..)"),
        }
    }
}

impl Description {
    /// Resolves the description.
    pub async fn resolve(&self, ctx: DescriptionContext) -> String {
        match self {
            Self::Static(text) => text.clone(),
            Self::Dynamic(function) => function(ctx).await,
        }
    }

    /// The static text, if the description is static.
    #[must_use]
    pub fn as_static(&self) -> Option<&str> {
        match self {
            Self::Static(text) => Some(text),
            Self::Dynamic(_) => None,
        }
    }
}

/// Decides per call whether approval is needed.
pub type ApprovalFn = Arc<dyn Fn(JsonValue, ToolContext) -> BoxFuture<'static, bool> + Send + Sync>;

/// Tool-level approval declaration. Call-level approval policies configured
/// on the generation take precedence over it.
#[derive(Clone, Default)]
#[non_exhaustive]
pub enum NeedsApproval {
    /// Never ask.
    #[default]
    Never,
    /// Always ask.
    Always,
    /// Ask when the function returns `true` for the validated input.
    Dynamic(ApprovalFn),
}

impl fmt::Debug for NeedsApproval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Never => f.write_str("Never"),
            Self::Always => f.write_str("Always"),
            Self::Dynamic(_) => f.write_str("Dynamic(..)"),
        }
    }
}

impl NeedsApproval {
    /// Resolves the declaration for one call.
    pub async fn resolve(&self, input: JsonValue, ctx: ToolContext) -> bool {
        match self {
            Self::Never => false,
            Self::Always => true,
            Self::Dynamic(function) => function(input, ctx).await,
        }
    }

    /// Returns `true` unless the declaration is [`NeedsApproval::Never`].
    #[must_use]
    pub fn is_declared(&self) -> bool {
        !matches!(self, Self::Never)
    }
}

/// Called when the model starts producing input for the tool.
pub type InputStartHook = Arc<dyn Fn(ToolContext) -> BoxFuture<'static, ()> + Send + Sync>;
/// Called for each streamed input delta.
pub type InputDeltaHook = Arc<dyn Fn(String, ToolContext) -> BoxFuture<'static, ()> + Send + Sync>;
/// Called once the full input is available and valid.
pub type InputAvailableHook =
    Arc<dyn Fn(JsonValue, ToolContext) -> BoxFuture<'static, ()> + Send + Sync>;

/// Lifecycle hooks around tool input.
#[derive(Clone, Default)]
pub struct ToolHooks {
    /// See [`InputStartHook`].
    pub on_input_start: Option<InputStartHook>,
    /// See [`InputDeltaHook`].
    pub on_input_delta: Option<InputDeltaHook>,
    /// See [`InputAvailableHook`].
    pub on_input_available: Option<InputAvailableHook>,
}

impl fmt::Debug for ToolHooks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ToolHooks")
            .field("on_input_start", &self.on_input_start.is_some())
            .field("on_input_delta", &self.on_input_delta.is_some())
            .field("on_input_available", &self.on_input_available.is_some())
            .finish()
    }
}

/// Arguments of a [`ToModelOutputFn`].
#[derive(Debug, Clone, Copy)]
pub struct ModelOutputArgs<'a> {
    /// The tool call id.
    pub tool_call_id: &'a ToolCallId,
    /// The validated input.
    pub input: &'a JsonValue,
    /// The execution output.
    pub output: &'a JsonValue,
}

/// Converts an execution output into what the model receives.
pub type ToModelOutputFn = Arc<dyn Fn(ModelOutputArgs<'_>) -> ToolResultOutput + Send + Sync>;

/// A tool definition. Built with [`Tool::function`], [`Tool::dynamic`],
/// [`Tool::provider_defined`] or [`Tool::provider_executed`].
#[derive(Clone)]
pub struct Tool {
    pub(crate) kind: ToolKind,
    pub(crate) description: Option<Description>,
    pub(crate) title: Option<String>,
    pub(crate) input_schema: Schema<JsonValue>,
    pub(crate) output_schema: Option<Schema<JsonValue>>,
    pub(crate) context_schema: Option<Schema<JsonValue>>,
    pub(crate) execute: Option<Arc<dyn ToolExecute>>,
    pub(crate) needs_approval: NeedsApproval,
    pub(crate) strict: Option<bool>,
    pub(crate) input_examples: Vec<JsonObject>,
    pub(crate) metadata: Option<JsonObject>,
    pub(crate) provider_options: Option<ProviderOptions>,
    pub(crate) hooks: ToolHooks,
    pub(crate) to_model_output: Option<ToModelOutputFn>,
    pub(crate) caller_definition: Option<ToolCallerDefinition>,
}

impl fmt::Debug for Tool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Tool")
            .field("kind", &self.kind)
            .field("description", &self.description)
            .field("title", &self.title)
            .field("input_schema", self.input_schema.json_schema())
            .field("has_output_schema", &self.output_schema.is_some())
            .field("has_context_schema", &self.context_schema.is_some())
            .field("executable", &self.execute.is_some())
            .field("needs_approval", &self.needs_approval)
            .field("strict", &self.strict)
            .field("input_examples", &self.input_examples)
            .field("metadata", &self.metadata)
            .field("provider_options", &self.provider_options)
            .field("hooks", &self.hooks)
            .field("has_to_model_output", &self.to_model_output.is_some())
            .field("caller_definition", &self.caller_definition)
            .finish()
    }
}

impl Tool {
    /// The kind.
    #[must_use]
    pub fn kind(&self) -> &ToolKind {
        &self.kind
    }

    /// The description declaration.
    #[must_use]
    pub fn description(&self) -> Option<&Description> {
        self.description.as_ref()
    }

    /// The title.
    #[must_use]
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The input schema (erased to JSON).
    #[must_use]
    pub fn input_schema(&self) -> &Schema<JsonValue> {
        &self.input_schema
    }

    /// The output schema, if declared.
    #[must_use]
    pub fn output_schema(&self) -> Option<&Schema<JsonValue>> {
        self.output_schema.as_ref()
    }

    /// The context schema, if declared.
    #[must_use]
    pub fn context_schema(&self) -> Option<&Schema<JsonValue>> {
        self.context_schema.as_ref()
    }

    /// The executor, if the tool is executed by the application.
    #[must_use]
    pub fn executor(&self) -> Option<&Arc<dyn ToolExecute>> {
        self.execute.as_ref()
    }

    /// Returns `true` when the tool has an executor.
    #[must_use]
    pub fn is_executable(&self) -> bool {
        self.execute.is_some()
    }

    /// The approval declaration.
    #[must_use]
    pub fn needs_approval(&self) -> &NeedsApproval {
        &self.needs_approval
    }

    /// Strict-mode request.
    #[must_use]
    pub fn strict(&self) -> Option<bool> {
        self.strict
    }

    /// Example inputs.
    #[must_use]
    pub fn input_examples(&self) -> &[JsonObject] {
        &self.input_examples
    }

    /// Metadata propagated to tool calls (not sent to the model).
    #[must_use]
    pub fn metadata(&self) -> Option<&JsonObject> {
        self.metadata.as_ref()
    }

    /// Provider options sent with the tool definition.
    #[must_use]
    pub fn provider_options(&self) -> Option<&ProviderOptions> {
        self.provider_options.as_ref()
    }

    /// Input lifecycle hooks.
    #[must_use]
    pub fn hooks(&self) -> &ToolHooks {
        &self.hooks
    }

    /// Custom model-output conversion.
    #[must_use]
    pub fn to_model_output(&self) -> Option<&ToModelOutputFn> {
        self.to_model_output.as_ref()
    }

    /// Caller definition, when this tool can call other tools.
    #[must_use]
    pub fn caller_definition(&self) -> Option<&ToolCallerDefinition> {
        self.caller_definition.as_ref()
    }

    /// Returns a copy with different provider options.
    #[must_use]
    pub fn with_provider_options(mut self, provider_options: Option<ProviderOptions>) -> Self {
        self.provider_options = provider_options;
        self
    }

    /// Resolves the description for a call (`None` when the tool has none).
    pub async fn resolve_description(&self, ctx: DescriptionContext) -> Option<String> {
        match &self.description {
            Some(description) => Some(description.resolve(ctx).await),
            None => None,
        }
    }

    /// Builds the provider-facing definition under `name`.
    #[must_use]
    pub fn definition(&self, name: ToolName, description: Option<String>) -> ToolDefinition {
        match &self.kind {
            ToolKind::Function | ToolKind::Dynamic => ToolDefinition::Function {
                name,
                description,
                input_schema: self.input_schema.json_schema().clone(),
                strict: self.strict,
                input_examples: self.input_examples.clone(),
                provider_options: self.provider_options.clone(),
            },
            ToolKind::ProviderDefined { id, args }
            | ToolKind::ProviderExecuted { id, args, .. } => ToolDefinition::Provider {
                id: id.clone(),
                name,
                args: args.clone(),
            },
        }
    }

    /// Validates an input against the input schema.
    ///
    /// # Errors
    ///
    /// Returns the validation error with `field: "tool input"` and the tool
    /// name as entity.
    pub fn validate_input(
        &self,
        name: &ToolName,
        input: JsonValue,
    ) -> Result<JsonValue, TypeValidationError> {
        self.input_schema.validate(input).map_err(|error| {
            error.with_context(TypeValidationContext {
                field: Some("tool input".to_owned()),
                entity_name: Some(name.as_str().to_owned()),
                entity_id: None,
            })
        })
    }

    /// Validates the selected tool context, preserving it when no schema is set.
    ///
    /// # Errors
    ///
    /// Returns the validation error with `field: "tool context"`.
    pub fn validate_context(
        &self,
        name: &ToolName,
        context: Option<JsonValue>,
    ) -> Result<Option<JsonValue>, TypeValidationError> {
        let Some(schema) = &self.context_schema else {
            return Ok(context);
        };
        schema
            .validate(context.unwrap_or(JsonValue::Null))
            .map(Some)
            .map_err(|error| {
                error.with_context(TypeValidationContext {
                    field: Some("tool context".to_owned()),
                    entity_name: Some(name.as_str().to_owned()),
                    entity_id: None,
                })
            })
    }

    /// Selects this tool's entry from a named context map and validates it.
    ///
    /// The map is keyed by the registered tool name. Missing entries remain
    /// absent without a schema and are validated as JSON `null` with a schema.
    ///
    /// # Errors
    ///
    /// Returns a validation error for a non-object context map or when the
    /// selected entry does not satisfy this tool's context schema.
    pub fn validate_named_context(
        &self,
        name: &ToolName,
        contexts: Option<&JsonValue>,
    ) -> Result<Option<JsonValue>, TypeValidationError> {
        if let Some(contexts) = contexts
            && !contexts.is_object()
            && !contexts.is_null()
        {
            return Err(TypeValidationError::new(
                contexts.clone(),
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "tools context must be an object keyed by tool name",
                ),
            )
            .with_context(TypeValidationContext {
                field: Some("tools context".to_owned()),
                entity_name: Some(name.as_str().to_owned()),
                entity_id: None,
            }));
        }
        self.validate_context(
            name,
            contexts.and_then(|value| value.get(name.as_str())).cloned(),
        )
    }

    /// Starts an execution; `None` when the tool has no executor.
    #[must_use]
    pub fn execute(&self, input: JsonValue, ctx: ToolContext) -> Option<ToolOutputStream> {
        self.execute
            .as_ref()
            .map(|executor| executor.execute(input, ctx))
    }
}

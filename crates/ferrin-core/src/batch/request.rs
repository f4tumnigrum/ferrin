//! Batch request types and their validation.

use std::collections::HashMap;

use ferrin_message::Message;
use ferrin_spec::AspectRatio;
use ferrin_spec::ImageSize;
use ferrin_spec::ModelId;
use ferrin_spec::ProviderOptions;
use ferrin_spec::ResponseFormat;
use ferrin_spec::ToolChoice;
use ferrin_spec::ToolDefinition;
use ferrin_spec::ToolName;
use ferrin_spec::image_model::ImageFile;
use ferrin_tool::ToolSet;

use crate::error::Error;
use crate::prompt::CallSettings;
use crate::prompt::Instructions;

/// A text generation request of a batch (the settings part of
/// `generate_text`, without callbacks or cancellation).
#[derive(Debug, Clone)]
pub struct TextBatchRequest {
    /// Request id, unique within the batch.
    pub id: String,
    /// Model id at the provider.
    pub model_id: ModelId,
    /// System instructions.
    pub system: Option<Instructions>,
    /// Single user prompt (exclusive with `messages`).
    pub prompt: Option<String>,
    /// Conversation (exclusive with `prompt`).
    pub messages: Option<Vec<Message>>,
    /// Whether system messages may appear inside `messages`.
    pub allow_system_in_messages: bool,
    /// Tools offered to the model (the batch provider cannot execute them).
    pub tools: ToolSet,
    /// Tool choice.
    pub tool_choice: Option<ToolChoice>,
    /// Tools sent to the model (all when `None`).
    pub active_tools: Option<Vec<ToolName>>,
    /// Tool order.
    pub tool_order: Vec<ToolName>,
    /// Shared tool context used to resolve dynamic descriptions.
    pub tools_context: Option<ferrin_spec::JsonValue>,
    /// Sampling settings and provider options.
    pub settings: CallSettings,
    /// Response format.
    pub response_format: Option<ResponseFormat>,
}

impl TextBatchRequest {
    /// Creates a request for `model_id`.
    #[must_use]
    pub fn new(id: impl Into<String>, model_id: impl Into<ModelId>) -> Self {
        Self {
            id: id.into(),
            model_id: model_id.into(),
            system: None,
            prompt: None,
            messages: None,
            allow_system_in_messages: false,
            tools: ToolSet::new(),
            tool_choice: None,
            active_tools: None,
            tool_order: Vec::new(),
            tools_context: None,
            settings: CallSettings::default(),
            response_format: None,
        }
    }

    /// Sets the system instructions.
    #[must_use]
    pub fn system(mut self, system: impl Into<Instructions>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Sets a single user prompt.
    #[must_use]
    pub fn prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = Some(prompt.into());
        self
    }

    /// Sets the conversation.
    #[must_use]
    pub fn messages(mut self, messages: impl IntoIterator<Item = Message>) -> Self {
        self.messages = Some(messages.into_iter().collect());
        self
    }

    /// Allows system messages inside the conversation.
    #[must_use]
    pub fn allow_system_in_messages(mut self, allow: bool) -> Self {
        self.allow_system_in_messages = allow;
        self
    }

    /// Sets the tools.
    #[must_use]
    pub fn tools(mut self, tools: ToolSet) -> Self {
        self.tools = tools;
        self
    }

    /// Sets the tool choice.
    #[must_use]
    pub fn tool_choice(mut self, tool_choice: ToolChoice) -> Self {
        self.tool_choice = Some(tool_choice);
        self
    }

    /// Restricts the tools sent to the model.
    #[must_use]
    pub fn active_tools(
        mut self,
        active_tools: impl IntoIterator<Item = impl Into<ToolName>>,
    ) -> Self {
        self.active_tools = Some(active_tools.into_iter().map(Into::into).collect());
        self
    }

    /// Sets the tool order.
    #[must_use]
    pub fn tool_order(mut self, tool_order: impl IntoIterator<Item = impl Into<ToolName>>) -> Self {
        self.tool_order = tool_order.into_iter().map(Into::into).collect();
        self
    }

    /// Sets the shared tool context.
    #[must_use]
    pub fn tools_context(mut self, context: ferrin_spec::JsonValue) -> Self {
        self.tools_context = Some(context);
        self
    }

    /// Sets the sampling settings and provider options.
    #[must_use]
    pub fn settings(mut self, settings: CallSettings) -> Self {
        self.settings = settings;
        self
    }

    /// Sets the response format.
    #[must_use]
    pub fn response_format(mut self, response_format: ResponseFormat) -> Self {
        self.response_format = Some(response_format);
        self
    }

    /// Sets the provider options.
    #[must_use]
    pub fn provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.settings.provider_options = provider_options;
        self
    }
}

/// An image generation request of a batch.
#[derive(Debug, Clone)]
pub struct ImageBatchRequest {
    /// Request id, unique within the batch.
    pub id: String,
    /// Model id at the provider.
    pub model_id: ModelId,
    /// Text prompt.
    pub prompt: Option<String>,
    /// Number of images.
    pub n: u32,
    /// Image size.
    pub size: Option<ImageSize>,
    /// Aspect ratio.
    pub aspect_ratio: Option<AspectRatio>,
    /// Seed.
    pub seed: Option<u64>,
    /// Reference images.
    pub files: Vec<ImageFile>,
    /// Mask.
    pub mask: Option<ImageFile>,
    /// Provider options.
    pub provider_options: ProviderOptions,
}

impl ImageBatchRequest {
    /// Creates a request for `model_id`.
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        model_id: impl Into<ModelId>,
        prompt: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            model_id: model_id.into(),
            prompt: Some(prompt.into()),
            n: 1,
            size: None,
            aspect_ratio: None,
            seed: None,
            files: Vec::new(),
            mask: None,
            provider_options: ProviderOptions::new(),
        }
    }

    /// Number of images (default 1).
    #[must_use]
    pub fn n(mut self, n: u32) -> Self {
        self.n = n;
        self
    }

    /// Image size.
    #[must_use]
    pub fn size(mut self, size: ImageSize) -> Self {
        self.size = Some(size);
        self
    }

    /// Aspect ratio.
    #[must_use]
    pub fn aspect_ratio(mut self, aspect_ratio: AspectRatio) -> Self {
        self.aspect_ratio = Some(aspect_ratio);
        self
    }

    /// Seed.
    #[must_use]
    pub fn seed(mut self, seed: u64) -> Self {
        self.seed = Some(seed);
        self
    }

    /// Reference images.
    #[must_use]
    pub fn files(mut self, files: Vec<ImageFile>) -> Self {
        self.files = files;
        self
    }

    /// Mask.
    #[must_use]
    pub fn mask(mut self, mask: ImageFile) -> Self {
        self.mask = Some(mask);
        self
    }

    /// Provider options.
    #[must_use]
    pub fn provider_options(mut self, provider_options: ProviderOptions) -> Self {
        self.provider_options = provider_options;
        self
    }
}

/// A request of a batch.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum BatchRequest {
    /// Text generation.
    Text(Box<TextBatchRequest>),
    /// Image generation.
    Image(Box<ImageBatchRequest>),
}

impl BatchRequest {
    /// The request id.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Text(request) => &request.id,
            Self::Image(request) => &request.id,
        }
    }
}

impl From<TextBatchRequest> for BatchRequest {
    fn from(request: TextBatchRequest) -> Self {
        Self::Text(Box::new(request))
    }
}

impl From<ImageBatchRequest> for BatchRequest {
    fn from(request: ImageBatchRequest) -> Self {
        Self::Image(Box::new(request))
    }
}

pub(super) fn validate_requests(requests: &[BatchRequest]) -> Result<(), Error> {
    if requests.is_empty() {
        return Err(Error::invalid_argument("requests", "must not be empty"));
    }
    let mut ids: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for request in requests {
        let id = request.id();
        if id.trim().is_empty() {
            return Err(Error::invalid_argument(
                "requests",
                "request ids must not be empty",
            ));
        }
        if !ids.insert(id) {
            return Err(Error::invalid_argument(
                "requests",
                format!("request ids must be unique; duplicate id `{id}`"),
            ));
        }
    }
    Ok(())
}

pub(super) fn validate_compatible_tools(
    request_id: &str,
    definitions: &[ToolDefinition],
    seen: &mut HashMap<ToolName, ToolDefinition>,
) -> Result<(), Error> {
    for definition in definitions {
        let name = match definition {
            ToolDefinition::Function { name, .. } | ToolDefinition::Provider { name, .. } => {
                name.clone()
            }
            #[allow(unreachable_patterns, reason = "ToolDefinition is non-exhaustive")]
            _ => continue,
        };
        if let Some(previous) = seen.get(&name)
            && previous != definition
        {
            return Err(Error::invalid_argument(
                "requests",
                format!(
                    "tool `{name}` must have the same definition in every batch request \
                     (request `{request_id}` differs)"
                ),
            ));
        }
        seen.insert(name, definition.clone());
    }
    Ok(())
}

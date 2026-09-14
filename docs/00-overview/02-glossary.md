# Glossary

**English** | [Chinese](../zh-CN/00-overview/02-glossary.md)

| Term | Definition | Ferrin identifier |
| --- | --- | --- |
| Provider | A service offering model capabilities and its adapter, which translates specification calls into concrete API requests. | `ferrin_spec::Provider` |
| Provider specification | Traits and data types that provider adapters must implement. A specification version change changes the adapter contract. | `ferrin_spec::SPEC_VERSION` |
| Language model | A model that accepts a prompt and generates text, reasoning, tool calls, and other content. | `ferrin_spec::LanguageModel` |
| Prompt (specification layer) | A normalized message sequence sent to a language model, containing only specification-defined content parts. | `ferrin_spec::Prompt` |
| Application message | An application-constructed message accepting convenient forms (strings, bytes, URLs), normalized into a specification prompt. | `ferrin_message::Message` |
| Content part | The smallest content unit in a message: text, file, reasoning, tool call, tool result, approval request, custom content, or source. | `*Part` types |
| Step | One model call and its subsequent tool execution in the generation loop. | `ferrin_core::StepResult` |
| Stop condition | A predicate deciding whether the generation loop continues when tool results are present. | `ferrin_core::StopCondition` |
| Tool | A model-callable function or provider capability with an input schema, optional execution function, and metadata. | `ferrin_tool::Tool` |
| Tool set | An ordered mapping from tool names to tools. | `ferrin_tool::ToolSet` |
| Provider-executed tool | A tool executed on the provider's server, such as web search, whose result arrives with the model response. | `ToolKind::ProviderExecuted` |
| Provider-defined tool | A tool whose schema is defined by the provider but executes on the client, such as computer use. | `ToolKind::ProviderDefined` |
| Dynamic tool | A tool whose schema is known only at runtime, such as an MCP tool; inputs and outputs are JSON values. | `ToolKind::Dynamic` |
| Tool approval | Application or user confirmation required before tool execution, represented by approval request and response parts. | `NeedsApproval`, `ToolApprovalRequestPart` |
| Deferred result | A provider-executed tool result missing from the current response that must arrive in a later step. | `supports_deferred_results` |
| Structured output | Model output constrained to schema-conforming JSON and parsed into a typed value. | `ferrin_core::Output` |
| Partial JSON repair | Completing incomplete streaming JSON to obtain a parseable intermediate value. | `ferrin_schema::partial_json` |
| Middleware | A language-model wrapper that can transform parameters and wrap generation and streaming calls. | `ferrin_core::LanguageModelMiddleware` |
| Registry | A component resolving model instances from `provider_id:model_id` strings. | `ferrin_core::ProviderRegistry` |
| Telemetry integration | A pluggable component receiving generation lifecycle callbacks. | `ferrin_core::Telemetry` |
| Warning | A report of missing capabilities, compatibility fallback, or deprecation from a provider or the core, without interrupting the call. | `ferrin_spec::Warning` |
| Usage | Token statistics for one or more model calls. | `ferrin_spec::Usage` |
| Finish reason | Why the model stopped generating, including a normalized enum and the provider's raw value. | `ferrin_spec::FinishReason` |
| Provider options | Passthrough JSON objects grouped by provider key for provider-specific request parameters. | `ProviderOptions` |
| Provider metadata | Passthrough JSON objects grouped by provider key for provider-specific response information. | `ProviderMetadata` |
| Provider reference | A mapping of provider-side resource identifiers, such as uploaded file IDs. | `ProviderReference` |
| MCP | Model Context Protocol, exposing tools, resources, and prompts over JSON-RPC. | `ferrin_mcp` |
| Cancellation token | A handle for cooperative cancellation of async operations. | `tokio_util::sync::CancellationToken` |
| Fixture | A recorded raw provider response, including SSE chunks, used for offline replay tests. | `tests/fixtures/` |

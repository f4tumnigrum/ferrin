# Agent

**English** | [Chinese](../zh-CN/01-architecture/09-agent.md)

Agent abstractions live in `ferrin-core::agent`.

## 1. Agent trait

[Decision] `Agent` packages a model, `tools`, and settings into a reusable object exposing `id`, `tools`, `generate(...)`, and `stream(...)`. Calls accept `prompt | messages`, optional typed `options`, cancellation, `timeout`, lifecycle hooks, and a sandbox; streaming also accepts transforms. Repeated calls with fixed model/tool configuration should declare it once.

```rust
pub trait Agent: Send + Sync {
    type Options: Send + 'static;                       // () when the agent takes no options (2026-09-13: no DeserializeOwned bound, see §6)
    type Output: Send + 'static;                        // () when no structured output

    fn id(&self) -> Option<&str>;
    fn tools(&self) -> &ToolSet;

    fn generate(&self, call: AgentCall<Self::Options>)
        -> impl Future<Output = Result<GenerateTextResult<Self::Output>, Error>> + Send;

    fn stream(&self, call: AgentStreamCall<Self::Options>)
        -> impl Future<Output = Result<StreamTextResult<Self::Output>, Error>> + Send;
}

pub struct AgentCall<O> {
    pub input: AgentInput,            // Prompt(String) | Messages(Vec<Message>)
    pub options: O,
    pub cancellation: CancellationToken,
    pub timeout: Option<Timeout>,
    pub hooks: Hooks,
    pub sandbox: Option<Arc<dyn Sandbox>>,
}
```

[Decision] Omit a `version` field; crate versions carry versioning, as in [Provider specification](04-provider-spec.md), section 1.

## 2. ToolLoopAgent

[Decision] `ToolLoopAgent` behavior:

- Settings `include` `model`, `instructions` (system prompt), `allow_system_in_messages`, `tools`, `tool_choice`, `stop_when` (default `step_count(20)`), `telemetry`, `active_tools`, `tool_order`, `output`, `runtime_context`, `tool_approval`, `tool_callers`, `tool_approval_secret`, `prepare_step`, `repair_tool_call`, `refine_tool_input`, all lifecycle hooks, `provider_options`, `download`, `include`, `prepare_call`, and all sampling/request options.
- `prepare_call` derives settings from call options, for example localized `instructions`. Explicitly cleared fields remove outer settings.
- Merge agent-level hooks before call-level hooks, preserving order.
- Append User-Agent suffix `ferrin-agent/tool-loop`.
- Delegate `generate` and `stream` to `generate_text` and `stream_text`.

```rust
pub struct ToolLoopAgent<Opt = (), Out = ()> { settings: Arc<ToolLoopAgentSettings<Opt, Out>> }

impl ToolLoopAgent {
    pub fn builder(model: impl Into<LanguageModelRef>) -> ToolLoopAgentBuilder<(), ()>;
}

impl<Opt, Out> ToolLoopAgentBuilder<Opt, Out> {
    pub fn id(self, id: impl Into<String>) -> Self;
    pub fn instructions(self, instructions: impl Into<Instructions>) -> Self;
    pub fn tools(self, tools: ToolSet) -> Self;
    pub fn stop_when(self, condition: impl StopCondition + 'static) -> Self;
    pub fn output<T>(self, output: Output<T>) -> ToolLoopAgentBuilder<Opt, T>;
    pub fn call_options<O: Send + 'static>(self) -> ToolLoopAgentBuilder<O, Out>;   // 2026-09-13: bounds relaxed, see §6
    pub fn prepare_call(self, f: impl Fn(PrepareCallInput<Opt>) -> BoxFuture<'static, Result<PreparedCall, Error>> + Send + Sync + 'static) -> Self;
    pub fn prepare_step(self, f: impl PrepareStep + 'static) -> Self;
    pub fn tool_approval(self, policy: impl ApprovalPolicy + 'static) -> Self;
    pub fn tool_approval_secret(self, secret: SecretBox<[u8]>) -> Self;
    pub fn telemetry(self, options: TelemetryOptions) -> Self;
    // sampling parameters, request options, hooks, provider_options, include, download ...
    pub fn build(self) -> ToolLoopAgent<Opt, Out>;
}
```

[Decision] Represent call options with generic `Opt`, originally requiring `DeserializeOwned + JsonSchema`, with `call_options::<O>()` also supplying a schema. The original design assumed runtime JSON requiring deserialization/validation. (Revised 2026-09-13: remove both bounds; see section 6 and [ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md), item 1.)

[Decision] Chain `ferrin-agent/tool-loop` with core `ferrin/<version>` and adapter `ferrin-<provider>/<version>` User-Agent suffixes, identifying SDK, adapter, and invocation form in provider usage statistics.

## 3. Example

```rust
#[derive(Deserialize, JsonSchema)]
struct SupportOptions { customer_tier: String }

let agent = ToolLoopAgent::builder(&model)
    .id("support-agent")
    .instructions("You are a support agent. Use tools to look up orders before answering.")
    .tools(support_tools())
    .call_options::<SupportOptions>()
    .prepare_call(|input| Box::pin(async move {
        let mut prepared = input.defaults;
        if input.options.customer_tier == "enterprise" {
            prepared.instructions = Some("You are a senior support agent. Prioritize SLA commitments.".into());
        }
        Ok(prepared)
    }))
    .stop_when(step_count(12))
    .build();

let result = agent
    .generate(AgentCall::prompt("Where is order #4521?").options(SupportOptions { customer_tier: "enterprise".into() }))
    .await?;
```

## 4. Custom agents

Applications may implement `Agent` to combine models or external planning. The core assumes no internal structure. An agent can be nested as a tool executor (sub-agent pattern), typically returning its `GenerateTextResult::text()` as tool output.

## 5. Verification items

- [Decision] (PV-009, `verification/pv009-override`; superseded 2026-09-13 by [ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md), item 2; see section 6) Overridable `PreparedCall` fields originally used `Override<T>::{Keep, Clear, Set(T)}`, defaulting to `Keep`, with `From<T>` → `Set` and `From<Option<T>>` mapping `None` → `Clear`. The prototype found `..PreparedCall::default()` with `Override::Clear` or `0.2.into()` readable, whereas nested `Option<Option<T>>` required remembering two meanings of `None` (`nested_option_is_ambiguous_to_read`).

## 6. Implementation record (2026-09-13)

- [Decision] ([ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md), item 1) `Agent::Options: Send + 'static`; `call_options::<O>()` requires neither `DeserializeOwned` nor `JsonSchema`.
- [Decision] (ADR 0013, item 2) `PrepareCall<Opt>::prepare_call(&self, PrepareCallInput<Opt> { options, defaults: PreparedCall }) -> BoxFuture<'_, Result<PreparedCall, Error>>`. `defaults` holds merged effective agent/call values: `instructions`, `model`, `tools`, `tool_choice`, `active_tools`, `tool_order`, `tools_context`, `settings: CallSettings`, `stop_conditions`, `timeout`, `retry_policy`, `include`, `max_tool_concurrency`, and `telemetry`. Setting a field to `None` removes it. Closures returning `Future<Output = Result<PreparedCall, Error>> + Send + 'static` implement the trait through a blanket implementation.
- [Decision] Default to `step_count(20)`. Call-level `timeout` replaces the entire agent `timeout` without field merging. `Hooks::merged` appends call hooks after agent hooks. Default empty `telemetry.function_id` to the agent ID. Twenty steps bound ordinary loops; agent hooks log before call-specific behavior; whole-`timeout` replacement avoids ambiguous merging.
- [Fact] `AgentCall<O>` offers `new/prompt/messages` on `AgentCall<()>`, `options<P>()` to change the option type, `cancellation`, `timeout`, `hooks`, `on_step_end`, `on_end`, and `streaming() -> AgentStreamCall<O>`. `AgentStreamCall<O>` adds `transform`, `include_raw_chunks`, `stream_retries`, `on_error`, `on_chunk`, and `on_abort`. Append the agent User-Agent suffix before the core appends `ferrin/<version>`.

## 7. Implementation record (2026-09-17)

[Decision] The shared builder accepts `runtime_context(JsonValue)`, and `PreparedCall::runtime_context` lets `prepare_call` set or clear invocation state independently of `tools_context`. Agent settings are cloned for each invocation; step overrides mutate only that invocation's evolving state. Generation and streaming have identical retention behavior under [ADR 0021](../04-decisions/2026-09-17-0021-agent-runtime-context.md).

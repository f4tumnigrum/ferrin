# Tool system

**English** | [Chinese](../zh-CN/01-architecture/06-tool-system.md)

Tool definitions live in `ferrin-tool`; parsing, approval, execution, and repair live in `ferrin-core::generate_text`.

## 1. Tool kinds

[Decision] Tools are classified by kind:

| Kind | Characteristics | Executor | Schema source |
| --- | --- | --- | --- |
| Function | Default kind | Client (with `execute`) or application (without it) | Application |
| Dynamic | Runtime JSON input/output | Client | Runtime, such as MCP; inputs/outputs are `unknown` |
| Provider-defined | Defined by provider, executed locally | Client | Provider crate, such as Anthropic computer use |
| Provider-executed | Defined and executed by provider; optional deferred results | Provider server | Provider crate |

[Decision] Tool fields: `description` (string or function receiving `{context, sandbox}`), `input_schema`, `output_schema`, `context_schema`, `execute`, `needs_approval`, `strict`, `input_examples`, `metadata` (sent to the provider), `provider_options`, `on_input_start`/`on_input_delta`/`on_input_available`, and `to_model_output`.

```rust
pub struct Tool {
    kind: ToolKind,
    description: Description,                  // Static(String) | Dynamic(Arc<dyn Fn(&ToolDescriptionContext) -> BoxFuture<'_, String>>)
    input_schema: Schema<JsonValue>,           // typed tools erase to JsonValue here; typing lives in the executor
    output_schema: Option<Schema<JsonValue>>,
    context_schema: Option<Schema<JsonValue>>,
    execute: Option<Arc<dyn ToolExecute>>,
    needs_approval: NeedsApproval,             // Never | Always | Dynamic(fn)
    strict: Option<bool>,
    input_examples: Vec<JsonObject>,
    metadata: Option<JsonObject>,
    provider_options: Option<ProviderOptions>,
    hooks: ToolHooks,                          // on_input_start / delta / available
    to_model_output: Option<Arc<dyn Fn(&JsonValue) -> ToolResultOutput + Send + Sync>>,
}

#[non_exhaustive]
pub enum ToolKind {
    Function,
    Dynamic,
    ProviderDefined { id: String, args: JsonObject },
    ProviderExecuted { id: String, args: JsonObject, supports_deferred_results: bool },
}
```

### 1.1 Typed definitions

[Decision] Preserve types at definition time; use JSON values at runtime:

```rust
#[derive(Deserialize, JsonSchema)]
struct GetWeatherInput {
    /// City name, e.g. "Berlin".
    city: String,
}

#[derive(Serialize)]
struct Weather { temperature_c: f32, condition: String }

let get_weather = Tool::function::<GetWeatherInput>()
    .description("Get the current weather for a city.")
    .execute(|input: GetWeatherInput, _ctx: ToolContext| async move {
        Ok(Weather { temperature_c: 21.5, condition: "sunny".into() })
    })
    .build();

let tools = ToolSet::new().insert("get_weather", get_weather)?;
```

[Fact] The 2026-09-13 implementation (`crates/ferrin-tool/`) differs from the draft above:

- `Tool::description` is `Option<Description>`; descriptions are optional. Additional fields are `title: Option<String>` (included in fingerprints) and `caller_definition: Option<ToolCallerDefinition>` (section 5). `to_model_output` is `Fn(ModelOutputArgs { tool_call_id, input, output }) -> ToolResultOutput`.
- Constructors: `Tool::function::<I>()` (using `Schema::<I>::derived().erased()`), `Tool::function_with_schema(Schema<JsonValue>)`, `Tool::dynamic(schema)`, `Tool::provider_defined(id, args)`, and `Tool::provider_executed(id, args)` (with `.supports_deferred_results(true)`). `ToolBuilder<I>` offers `description`/`description_fn`, `title`, `input_schema`, `output_schema`, `context_schema`, `needs_approval`/`needs_approval_if`, `strict`, `input_example(s)`, `metadata`, `provider_options`, `on_input_start`/`on_input_delta`/`on_input_available`, `to_model_output`, `caller`, `execute` (async closure returning `Result<O: Serialize, ToolError>`), `execute_stream` (a `Stream<Item = Result<O, ToolError>>`, each item `Preliminary`, with the last repeated as `Final`), `execute_with(Arc<dyn ToolExecute>)`, and `build`.
- Methods: `definition(name, description) -> spec::ToolDefinition` (function/dynamic → `Function`; provider tools → `Provider`), `resolve_description(DescriptionContext)`, `validate_input(name, value)` (error context `field: "tool input"`), `validate_context(name, Option<JsonValue>)` (returns `None` without a context schema, validates missing context as `null` otherwise, error context `field: "tool context"`), and `execute(input, ctx) -> Option<ToolOutputStream>`. Typed closures deserialize validated JSON through serde; failure is `ToolError::Message("invalid tool input: ...")`.

`Tool::function::<I>()` requires `I: DeserializeOwned + JsonSchema`; execution output `O: Serialize` becomes `JsonValue` internally. Inferring an entire tool set into a Rust result type would require a per-set enum or macro. Tool-level typing, JSON results, and helpers such as `step.tool_result_as::<Weather>("get_weather")` balance usability and complexity.

[Decision] `ferrin-macros` provides `#[ferrin::tool]`, turning a documented `async fn` into a `Tool` constructor. Doc comments supply the description; the parameter struct supplies the input schema. This optional convenience layer generates only public API calls.

[Fact] Implementation on 2026-09-14 (`crates/ferrin-macros/`): `[pub] [async] fn name(input: I[, ctx: ToolContext]) -> Result<O, ToolError>` expands to `[pub] fn name() -> ::ferrin::tool::Tool`. The body remains an inner function, used by `Tool::function::<I>().description(<doc comments>).execute(|input, ctx| name(input[, ctx])).build()`. Synchronous functions use `core::future::ready`; absent docs omit the `description`. Strip the first space from each doc line, join with newlines, and trim outer blank lines; field docs enter schema descriptions through `schemars`. Reference types, including nested `Vec<&str>`, or explicit lifetimes produce `tool parameters must be owned types (use String instead of &str)`; type generics produce `tool functions cannot be generic`. Dedicated errors cover parameter counts other than 1 or 2, `self`, missing return types, `const`/`unsafe`/ABI/variadics, and macro arguments. Generated paths use `::ferrin::tool::*`, not `ferrin_tool`, so use the macro through the facade. Input derives need `#[serde(crate = "ferrin::serde")]` and `#[schemars(crate = "ferrin::schemars")]`, or direct `serde`/`schemars` dependencies. Tests: `crates/ferrin/tests/suite/tool_macro.rs` and `crates/ferrin/tests/ui/` (one pass and seven compile-fail cases).

### 1.2 Tool sets

[Decision] `ToolSet` indexes tools by names unique within the set.

`ToolSet` uses `IndexMap<ToolName, Arc<Tool>>`, preserving insertion order, with `insert`, `get`, `names`, `filter(active)`, and `merge`. Duplicate insertion returns an error instead of replacing a tool.

[Fact] Implementation on 2026-09-13: `insert(self, name, tool) -> Result<Self, DuplicateToolError>` (chaining requires `?`), `try_insert(&mut self, ..)`, `try_insert_arc`, `replace` (preserves position), `remove` (preserves other ordering), `get`, `contains`, `names`, `iter`, `len`/`is_empty`, `filter_active(&[ToolName])` (preserves set order, ignores unknown `names`), `merge(self, other) -> Result<Self, DuplicateToolError>`, and `ordered(&[ToolName])` (listed `names` first, then alphabetically, as in section 6). `ToolSet: Clone` shares `Arc<Tool>` values.

## 2. Execution contract

[Decision] Execution may return one value or a stream, whose intermediate values are `preliminary: true` and last value is final. `ToolContext` contains `tool_call_id`, `messages` (sent to the model in this step), cancellation, `tools_context`, and `sandbox`. Preliminary results let long-running search or code tools report progress before completion.

```rust
pub trait ToolExecute: Send + Sync {
    fn execute(&self, input: JsonValue, ctx: ToolContext) -> BoxStream<'static, Result<ToolOutput, ToolError>>;
}

pub enum ToolOutput {
    Preliminary(JsonValue),
    Final(JsonValue),
}

pub struct ToolContext {
    pub tool_call_id: ToolCallId,
    pub messages: Arc<[Message]>,
    pub cancellation: CancellationToken,
    pub tools_context: Option<JsonValue>,      // validated against context_schema
    pub sandbox: Option<Arc<dyn Sandbox>>,     // feature "sandbox"
}
```

Single-value executors adapt through `From` to a stream yielding one `Final`, allowing one core path for preliminary and final results.

[Fact] Implementation on 2026-09-13: `ToolExecute::execute(&self, JsonValue, ToolContext) -> ToolOutputStream` (`BoxStream<'static, Result<ToolOutput, ToolError>>`). `ToolContext` has the fields above; `sandbox` and `with_sandbox` exist only with feature `sandbox`. Start with `ToolContext::new(tool_call_id)`, then `with_messages`/`with_cancellation`/`with_tools_context`. `execute_to_completion(stream, on_preliminary)` returns the final value, or `ToolError::Message` if no `Final` arrives. Local `ToolError` (see [Error model](12-error-model.md), section 3) offers `message`, `json`, `from_error`, `with_cause`, `is_cancelled`, and `From<serde_json::Error>`. `model_output::create_tool_model_output(tool, tool_call_id, input, output, ErrorMode::{None, Text, Json})` and `tool_error_output(&ToolError)` normalize output (`Json` errors → `error-json`, others → `error-text`; `error_message` maps `null` to `unknown error` and uses an object's `message` field).

[Decision] Capture tool failures as nonfatal `tool-error` step content, then send `error-text`/`error-json` to the model. Handle cancellation separately. Models can often repair arguments from error text; terminating would lose this recovery path.

## 3. Tool-call parsing and repair

[Decision] Parsing rules:

1. Unknown tool names produce `NoSuchTool`; pass this error to `repair_tool_call` if configured.
2. Treat empty input as `{}`. Parse JSON and validate against `input_schema`; on failure, create `InvalidToolInput` and attempt repair.
3. A repair returning `None` gives up; a replacement call is reparsed; a repair failure is wrapped as a repair error.
4. Unparsable calls become content with `invalid: true, dynamic: true` and an `error`, rather than being thrown. Subsequent `tool-error` content reports the failure to the model.
5. Calling a different tool when `tool_choice` specifies a name is a tool-choice violation and becomes an invalid call.
6. Optional per-tool `refine_tool_input` rewrites validated input without changing its shape, for execution, events, and telemetry.
7. Provider-executed calls skip local schema validation; parse their input directly as JSON.

```rust
pub trait ToolCallRepair: Send + Sync {
    fn repair(
        &self,
        request: RepairRequest<'_>,     // { tool_call, tools, input_schema, messages, system, error: NoSuchTool | InvalidToolInput }
    ) -> BoxFuture<'_, Result<Option<spec::ToolCall>, Box<dyn Error + Send + Sync>>>;
}
```

## 4. Approval

### 4.1 Resolution

[Decision] Approval resolution:

- Call-level `ApprovalPolicy` takes precedence, then per-tool user configuration, then the tool's own boolean or function `needs_approval`.
- Four states: `not-applicable` (execute directly), `approved`, `denied {reason?}`, and `user-approval {reason?}` (external approval required).
- Calls with historical approval responses follow those responses.

```rust
pub enum ApprovalStatus {
    NotApplicable,
    Approved,
    Denied { reason: Option<String> },
    UserApproval { reason: Option<String> },
}

pub trait ApprovalPolicy: Send + Sync {
    fn resolve(&self, call: &ParsedToolCall, ctx: &ApprovalContext) -> BoxFuture<'_, ApprovalDecision>;
}
```

(2026-09-13: implemented as `resolve<'a>(&'a self, call: &'a ParsedToolCall, ctx: ApprovalContext<'a>) -> BoxFuture<'a, Option<ApprovalStatus>>`; see section 11.)

[Decision] Policies evaluated by an external or embedded engine (OPA REST Data API, Rego through `regorus`) plug in through the same trait: `ferrin_policy::policy_approval(client, path)` implements `ApprovalPolicy`, maps `allow` / `deny` / `requires-approval` to the statuses above and `not-applicable` to `None`, and denies on evaluation failure by default. See [Policy-based tool approval](18-policy-approval.md) and [ADR 0020](../04-decisions/2026-09-15-0020-policy-based-tool-approval.md).

### 4.2 Approval requests and responses

[Decision] Calls requiring user approval do not execute. They emit `tool-approval-request {approval_id, tool_call_id, tool_name, input, reason?, signature?}` and end the loop because continuation conditions fail. The application appends `tool-approval-response {approval_id, approved, reason?, provider_executed?}` to the last tool message and calls again; the signature remains with the request in the assistant message. The core:

1. Collects approval responses from the last tool message and finds matching requests and calls in historical assistant messages; missing calls produce `ToolCallNotFoundForApproval`.
2. Verifies signatures if an approval secret is configured; mismatch throws `InvalidToolApprovalError`.
3. Revalidates input and resolves approval policy again to guard against tampered history.
4. Executes approved calls and produces `execution-denied` results for denials.

[Decision] Sign with HMAC-SHA256 using application `tool_approval_secret`. The payload is `["ferrin-tool-approval-v1", approval_id, tool_call_id, tool_name, input_digest]`, with a SHA-256 digest of canonical input JSON; signatures use base64url. Domain separation prevents signature reuse; signing a digest fixes its input component's length, and canonicalization removes key-order dependence.

[Decision] Ferrin uses this structure with domain separator `"ferrin-tool-approval-v1"`, compact key-sorted JSON, and `secrecy::SecretBox<[u8]>` keys. Persisted message history requires tamper protection; product-specific domain separation prevents cross-system replay.

### 4.3 Tool fingerprints and drift

[Decision] `fingerprint_tools` digests tool names, descriptions, and schemas; `detect_tool_drift` reports additions, removals, and changes between sets. Approval spans requests and refers to the original definition; changes between requests should invalidate it.

`ferrin_tool::fingerprint` provides these functions, returning `ToolDrift { added, removed, changed }`.

[Fact] Implementation on 2026-09-13: `fingerprint_tools(&ToolSet) -> BTreeMap<ToolName, String>` hashes compact key-sorted JSON `{ description: {type: "string", value} | {type: "function"} | {type: "none"}, inputSchema, title? }` with SHA-256 and unpadded base64url. `canonical_json`/`hash_canonical` are public; omit absent `title`. Baselines are generated by Ferrin, so number formatting and missing-key rules need only remain stable within Ferrin. `detect_tool_drift(current, baseline)` returns all three categories alphabetically.

## 5. Caller restrictions

[Decision] `tool_callers` specifies permitted callers for each tool: a locally bound caller tool or the provider. Other calls are invalid. This prevents models from directly invoking privileged tools intended only for indirect use.

[Decision] Use `ToolCallers = HashMap<ToolName, Vec<ToolCaller>>` with `ToolCaller::{Provider, Tool(ToolName)}`.

[Decision] The 2026-09-13 implementation names direct invocation `ToolCaller::Direct` to avoid confusion with `ToolCallerDefinition::Provider` (provider-mediated calls). `ToolCallerDefinition::{Local(bind: Fn(ToolSet) -> Tool), Provider(prepare: Fn(Option<ProviderOptions>) -> ProviderOptions)}` attaches through `ToolBuilder::caller`. `callers::validate_tool_callers(&ToolSet, &ToolCallers)` rejects unknown tools or callers without definitions with `InvalidArgumentError { argument: "tool_callers" }`. `callers::prepare_tools_for_callers` returns `PreparedToolCallers { execution_tools, model_tools }`: local callees bind into their caller and leave the model tool set; provider callees get rewritten `provider_options`; tools with neither `Direct` nor a provider caller are omitted from model input.

## 6. Active tools and ordering

[Decision] `active_tools` restricts tools sent in this step without changing result types; `tool_order` controls their order. `prepare_step` may override both. Smaller per-step sets guide selection and reduce prompt size; stable ordering helps provider prompt caches.

## 7. Tool context

[Decision] `tools_context` supplies shared execution context validated against each tool's `context_schema`. Request-specific user or tenant information belongs in tool context rather than the prompt.

[Decision] Validate `tools_context: Option<JsonValue>` once per tool at call time; tools without a context schema receive `None`.

## 8. Sandbox

[Decision] `SandboxSession` offers `description`, `read_file` (byte stream), `read_binary_file`, `read_text_file` (encoding, line range), `write_file`/`write_binary_file`/`write_text_file`, `spawn` (returning `SandboxProcess {stdout, stderr, wait, kill}`), and `run` (wait and collect output). Command options include `command`, `working_directory`, `env`, and cancellation. This is the minimum file/process interface needed by tools; applications implement concrete container or remote sandboxes.

```rust
pub trait Sandbox: Send + Sync {
    fn description(&self) -> &str;
    fn read_file(&self, opts: ReadFileOptions) -> BoxFuture<'_, io::Result<Option<BoxStream<'static, io::Result<Bytes>>>>>;
    fn read_binary_file(&self, opts: ReadFileOptions) -> BoxFuture<'_, io::Result<Option<Bytes>>>;
    fn read_text_file(&self, opts: ReadTextFileOptions) -> BoxFuture<'_, io::Result<Option<String>>>;
    fn write_file(&self, opts: WriteFileOptions) -> BoxFuture<'_, io::Result<()>>;
    fn spawn(&self, opts: ProcessOptions) -> BoxFuture<'_, io::Result<Box<dyn SandboxProcess>>>;
    fn run(&self, opts: ProcessOptions) -> BoxFuture<'_, io::Result<ProcessResult>>;
}
```

Ferrin defines traits and `LocalProcessSandbox` for tests/examples only, explicitly documented as providing no isolation.

[Decision] Since 2026-09-13, `Sandbox`, `SandboxProcess`, and `LocalProcessSandbox` live in `ferrin_tool::sandbox`, behind `sandbox` (adding `bytes` and Tokio `process`/`fs`/`io-util`). `Sandbox` belongs to tool context and is referenced by `ToolContext::sandbox` and `DescriptionContext::sandbox`; `ferrin-testing` depends on the core and cannot host traits the core needs.

[Fact] Implemented signatures on 2026-09-13: `read_file -> Option<ByteStream>` (`BoxStream<'static, io::Result<Bytes>>`), `read_binary_file -> Option<Bytes>`, `read_text_file(ReadTextFileOptions { path, encoding, start_line, end_line, cancellation }) -> Option<String>`, `write_file(WriteFileOptions<ByteStream>)`, `write_binary_file(WriteFileOptions<Bytes>)`, `write_text_file(WriteFileOptions<String>)`, `spawn -> Box<dyn SandboxProcess>`, and `run -> ProcessResult { exit_code, stdout, stderr }`. `SandboxProcess` provides `pid`, single-use `take_stdout`/`take_stderr`, `wait -> i32`, and idempotent `kill`. Cancellation in option structs returns `io::ErrorKind::Interrupted` and terminates the process. `LocalProcessSandbox::new(root)` executes `/bin/sh -c` (`cmd /C` on Windows), resolves paths against `root`, and permits absolute paths and `..`. Text is UTF-8 only (other encodings return `Unsupported`); line ranges are one-based and inclusive, clipped at EOF.

## 9. Execution timeouts and telemetry

- `Timeout::tool` is the default; `Timeout::per_tool[name]` overrides it. Timeouts are nonfatal tool errors.
- Each execution invokes `Telemetry::on_tool_execution_start/end` and creates a `ferrin.tool` `tracing` span with `tool.name`, `tool.call_id`, and duration.
- Each tool result in a step carries `tool_execution_ms`.

## 10. Verification items

- [Fact] (PV-004, `verification/pv004-schema`) `schemars` 1.2.2 `SchemaSettings::draft07()` emits `"type": [T, "null"]` for optional primitives and `anyOf: [{$ref}, {type: null}]` for optional referenced types. Optional fields are absent from `required`; unit enums use `type: string` plus `enum`; internally tagged enums use `oneOf`, with a `required` tag and `const` in each branch. Definitions live in `definitions`; `additionalProperties` is not generated. OpenAI strict mode requires `additionalProperties: false`, every property in `required`, no `propertyNames`, and nullable types for optional fields.
- [Decision] `SchemaTransform::openai_strict()` recursively sets `additionalProperties: false`, removes `propertyNames`, adds all properties to `required`, and makes previously optional properties nullable (wrap the complete property schema in `anyOf: [original, {type: null}]`, preserving `enum`, `const`, and composition constraints on non-null values). JSON Pointer references to resources declared in the schema are relocated with moved nodes, including fragment-only, relative and absolute URIs; nested `$id` scopes and URI-encoded names are preserved without retrieving external resources. The PV-004 prototype records the derived schema shapes above; regression tests in `crates/ferrin-schema/tests/suite/transform.rs` verify complete-schema nullability for optional enum, const, and composition constraints (2026-09-15). Default tool schemas only set `additionalProperties: false`; strict transformation applies only for OpenAI `strict: true`.
- [Fact] (PV-005, `verification/pv005-static-capture`) Under `Tool::function` bounds (`F: Fn(I) -> Fut + Send + Sync + 'static, Fut: Future + Send + 'static`), capturing local references produces rustc `E0597: borrowed value does not live long enough ... argument requires that ... is borrowed for 'static`, pointing to the bound. Reference parameters in async functions produce `lifetime may not live long enough ... closure implements Fn, so references to captured variables can't escape the closure`. Both diagnostics point to user code.
- [Decision] `#[ferrin::tool]` expands to `Tool::function`, reusing these diagnostics, and rejects reference parameter types (`&T`, `&str`, `&[T]`) or explicit lifetimes syntactically with `compile_error!("tool parameters must be owned types (use String instead of &str)")`. This reports common errors at macro input. `trybuild` cases live in `crates/ferrin/tests/ui/` (see testing standards).

## 11. Implementation record (2026-09-13)

- [Decision] `ApprovalPolicy::resolve` returns `Option<ApprovalStatus>`; `None` defers to the tool's `needs_approval`. `ApprovalStatus` implements the policy as a constant; wrap synchronous `Fn(&ParsedToolCall, &ApprovalContext<'_>) -> Option<ApprovalStatus>` in `ApprovalPolicyFn`. `Option` directly expresses fallback priority without a separate `ApprovalDecision` type.
- [Decision] `PrepareStep` has a blanket implementation for synchronous `Fn(&PrepareStepContext<'_>) -> StepOverrides`; applications needing async logic implement the trait. Most uses simply switch models or tools by step number, so synchronous closures avoid `Box::pin(async move { .. })` boilerplate.
- [Fact] `RefineToolInputs` stores `Arc<dyn Fn(JsonValue) -> BoxFuture<'static, Result<JsonValue, Error>>>` by tool name, rewriting input after schema validation and before execution. The replacement updates `ParsedToolCall.input` and response messages.
- [Fact] `ToolApprovalRequestContent { approval_id, tool_call: ParsedToolCall, reason, is_automatic }` carries the full parsed call. `StepContent::ToolApprovalResponse(ToolApprovalResponseContent { approval_id, tool_call, approved, reason, provider_executed })` records replay decisions. Approval UIs need names and input without searching history again.
- [Fact] `DescriptionContext::with_tool_context(JsonValue)` resolves dynamic descriptions outside generation loops, including batches and realtime sessions.

[Decision] Parsing and repair use the same caller-filtered, active tool set sent to the provider for that step. Tools hidden behind a local caller remain available to that caller, but a direct model call cannot select them or trigger their input hooks.

[Decision] Approval replay deduplicates decisions by both approval ID and tool-call ID within one invocation. Conflicting approval or provider-execution decisions fail before any tool executes; repeated identical decisions schedule one execution. This does not provide cross-invocation exactly-once execution.

[Decision] Approval replay validates the current context for every approved executable client tool before starting any execution. Context construction propagates validation failures as `InvalidArgument` for `tools_context`; it never substitutes a missing context after validation fails.

[Decision] Strict schema transforms return an error for arbitrary-key dictionaries instead of closing them or returning an unsupported schema; see [ADR 0019](../04-decisions/2026-09-15-0019-fallible-schema-transforms.md). `apply`, `applied`, `to_openai_strict`, and `Schema::transformed` return `Result`; failed in-place transformations leave their input unchanged.

[Decision] Local sandbox cancellation is checked before process creation and remains active for file and process output streams. Each spawned process has a `JoinSet`-owned supervisor that kills and reaps it on cancellation even when the application has not called `wait`; dropping the process aborts that supervisor and kills its owned child. Cancelled reads return one `Interrupted` error and end.

[Fact] 2026-09-15: `ferrin-policy` builds `policy_approval`, `shadow`, `with_default` and `capability_middleware` on this contract without changes to the core; its coverage is listed in [Policy-based tool approval](18-policy-approval.md), section 7.

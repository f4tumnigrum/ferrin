# Provider implementation guide

**English** | [Chinese](../zh-CN/01-architecture/17-provider-implementation-guide.md)

This guide defines the shared structure and contracts for adapter authors implementing crates such as `ferrin-openai` and `ferrin-anthropic`.

## 1. Shared adapter structure

[Decision] Every provider follows this structure:

- Export `create_xxx(settings)` and `XxxProvider`, implementing `Provider` and offering API-family methods (`chat`, `responses`, `completion`, `embedding`, `image`, `transcription`, `speech`, `files`, `skills`, `batch`, `realtime`) and a provider-tool factory module.
- Settings: validated, slash-trimmed `base_url` with environment fallback (Anthropic normalizes bare origins to `/v1`), lazy `api_key`, `headers`, configurable `name`, `transport`, and `id_generator`.
- Models take `(model_id, config)`; configuration supplies `<name>.<family>` identity, URL/header construction, and transport.
- `build_request` converts `CallOptions` and collects `unsupported`-option/tool warnings, deserializing only the provider's options.
- Generation uses JSON helpers/handlers; streaming uses SSE and `stream_driver` to map events, beginning with `stream-start {warnings}`.
- JSON error handlers combine body types and message extraction; stream errors carry type-inferred status and retryability.
- Normalize finish reasons and usage while preserving `raw` values.

## 2. Crate skeleton

```rust
// ferrin-openai/src/lib.rs (implemented signatures, 2026-09-13)
pub struct OpenAiSettings {
    pub base_url: Option<Url>,                       // env OPENAI_BASE_URL
    pub api_key: Option<SecretString>,               // env OPENAI_API_KEY (lazy)
    pub organization: Option<String>,
    pub project: Option<String>,
    pub headers: Headers,
    pub name: Option<String>,                        // provider name override
    pub transport: Option<SharedTransport>,
    pub id_generator: Option<Arc<dyn IdGenerator>>,
}

pub fn create_openai(settings: OpenAiSettings) -> Result<OpenAiProvider, ProviderError>;

impl OpenAiProvider {
    pub fn from_config(config: SharedConfig) -> Self;
    pub fn config(&self) -> &SharedConfig;
    pub fn responses(&self, model_id: impl Into<ModelId>) -> OpenAiResponsesLanguageModel;
    pub fn chat(&self, model_id: impl Into<ModelId>) -> OpenAiChatLanguageModel;
    pub fn completion(&self, model_id: impl Into<ModelId>) -> OpenAiCompletionLanguageModel;
    pub fn embedding(&self, model_id: impl Into<ModelId>) -> OpenAiEmbeddingModel;
    pub fn image(&self, model_id: impl Into<ModelId>) -> OpenAiImageModel;
    pub fn speech(&self, model_id: impl Into<ModelId>) -> OpenAiSpeechModel;
    pub fn transcription(&self, model_id: impl Into<ModelId>) -> OpenAiTranscriptionModel;
    #[cfg(feature = "realtime")]
    pub fn speech_translation(&self, model_id: impl Into<ModelId>) -> OpenAiSpeechTranslationModel;
    pub fn files(&self) -> OpenAiFiles;
    pub fn skills(&self) -> OpenAiSkills;
    pub fn batch(&self) -> OpenAiBatch;
    pub fn realtime(&self) -> OpenAiRealtimeFactory;
    pub fn tools(&self) -> &OpenAiTools;             // provider-defined / provider-executed tool factories
}

impl Provider for OpenAiProvider { /* language_model = responses(); other methods return their respective Ref */ }
```

[Decision] `create_openai` returns `Result`, rejecting invalid base URLs immediately but loading credentials lazily. URLs are configuration errors; credentials may be supplied through request headers.

[Decision] Family methods return concrete models, exposing `prepare_request` and `config` for testing/composition. Callers convert to Ref with `into()` when needed; `Provider` already offers the dynamic form.

## 3. Implementing language models

1. Define API request `Serialize` and response/chunk `Deserialize` types, ignoring unknown fields and preserving provider field names through serde renaming.
2. Define provider options (`Deserialize + JsonSchema`) and parse with `parse_provider_options::<OpenAiResponsesOptions>("openai", &options.provider_options)`.
3. Implement `spec::Prompt -> Vec<ApiMessage>` conversion:
   - Resolve provider references with `resolve_provider_reference(reference, provider_key)`; unsupported references return `UnsupportedFunctionality`.
   - Pass URLs directly only when supported; the core otherwise inlines bytes.
   - Map text/JSON output to strings or structured content, errors to flagged results, denials to explanatory text, and content to multimodal arrays.
   - Warn and skip unsupported parts, such as assistant files, instead of failing.
4. Convert function tools and provider tools; accept only own-prefix provider IDs, warning otherwise. Use `ToolNameMapping` for invalid names and restore names in responses.
5. Build requests and warnings. Map reasoning to effort or budget. [Decision] Missing effort mappings warn `unsupported`; approximate mappings warn `compatibility`. Budget fractions of maximum output tokens are `minimal 2%`, `low 10%`, `medium 30%`, `high 60%`, `xhigh 90%`, minimum 1024. Proportions preserve relative strength; 1024 is Anthropic's documented minimum.
6. Generate with `post_json`/`json_response_handler`, mapping content, finish, usage, metadata, request body, and response ID/timestamp/model/headers/body.
7. `Stream` with `post_json`/`event_source_response_handler` and `async_stream` or a custom `Stream`. Emit StreamStart, first response metadata, text/reasoning/tool-input boundaries, and `Finish`. Parse failures become `Error`; `include_raw_chunks` inserts `Raw` before each parsed chunk.
8. Implement finish-reason and usage conversion modules.
9. Implement error body schemas, message extraction, and status/retryability overrides.

## 4. Provider tools

[Decision] Tool factories return provider-defined/executed `tools` with IDs such as `openai.web_search`, arguments, input/output schemas, and provider-specific parsing. Keep provider-documented tool knowledge inside adapters.

```rust
impl OpenAiTools {
    pub fn web_search(&self, config: WebSearchConfig) -> Tool;      // ProviderExecuted
    pub fn file_search(&self, config: FileSearchConfig) -> Tool;    // ProviderExecuted
    pub fn code_interpreter(&self, config: CodeInterpreterConfig) -> Tool;
    pub fn image_generation(&self, config: ImageGenerationConfig) -> Tool;
}
```

## 5. Testing requirements

[Decision] Use fixtures: `tests/fixtures/<area>/<case>.chunks.txt` for raw SSE events and `.response.json` for non-streaming responses. Reconstruct streams and snapshot specification events; see [Testing](../03-engineering/04-testing.md), section 1.

Provider crates must include:

1. At least one generation and streaming fixture per API family, covering text, tools, reasoning, errors, and usage.
2. `StreamContractChecker` assertions for ordering.
3. Insta request snapshots for each option combination.
4. Expected unsupported-option warnings.
5. Optional ignored `live_*` tests gated by provider key environment variables.

Record with `cargo xtask record-fixture --provider openai --case responses/tool-call`, making one authenticated call and stripping secrets/accounts from response headers before writing.

## 6. `ferrin-openai-compatible`

[Decision] `ferrin-openai-compatible` supplies reusable Chat/Completion/Embedding/Image models configured by name, URL, headers, transport, structured-output support, etc. Many endpoints such as DeepSeek/Together claim Chat compatibility; shared models plus switches avoid redundant adapters.

[Fact] Exports include `OpenAiCompatibleProvider`, `create_openai_compatible`, settings fields `name`, `base_url`, `api_key`, `api_key_env`, `headers`, `query_params`, `include_usage`, `supports_structured_outputs`, `supported_urls`, `error_structure`, `metadata_extractor`, `transform_request_body`, `convert_usage`, `max_embeddings_per_call`, `supports_parallel_calls`, `transport`, `id_generator`; concrete ChatLanguageModel, CompletionLanguageModel, EmbeddingModel, and ImageModel types sharing `OpenAiCompatibleConfig`; and `ErrorStructure`, `MetadataExtractor`/`StreamMetadataExtractor` traits. Names/settings were corrected against implementation on 2026-09-13; see section 11 and [OpenAI-compatible endpoints](../providers/openai-compatible.md).

## 7. New provider checklist

- [ ] Name the crate `ferrin-<provider>` and inherit workspace lints/version.
- [ ] Supply factory/settings and document environment variables.
- [ ] Implement `Provider`, using defaults for unsupported model types.
- [ ] Route all networking through `ferrin_provider_util::http`.
- [ ] Append `ferrin-<provider>/<version>` to User-Agent.
- [ ] Include fixtures, request snapshots, and contract checks.
- [ ] Document capabilities, option schemas, metadata, limitations, and warnings in both editions of `docs/providers/<provider>.md`.
- [ ] Add facade feature and re-export.
- [ ] Add a changelog entry.

## 8. Verification items

- [Decision] (PV-021) Retain `OpenAiConfig` switches `explicit_message_item_type` (Azure Foundry Responses requires explicit message type) and `supports_web_search_sources_include` (Bedrock Mantle rejects `web_search_call.action.sources` include). Configuration is easier to maintain than forks for known Responses differences.
- [Decision] Defaults are `false` and `true` respectively. The original compatible-crate Responses passthrough clause was withdrawn on 2026-09-13 because that crate has no Responses mode ([ADR 0014](../04-decisions/2026-09-13-0014-openai-compatible-model-families.md)). Keep both flags in `OpenAiConfig`; use `ferrin-openai` `base_url`/`name` for compatible Responses endpoints.
## 9. Implementation record (2026-09-13, ferrin-openai)

- [Fact] Modules: `config` (shared configuration, URLs/headers/WebSocket URLs), `capabilities` (reasoning/system modes/`flex`/`priority`), responses/{options,api_types,convert_prompt,convert_tool_results,convert_tools,request,output,stream,stream/items}, chat, `completion`, `embedding`, `image`, `speech`, `transcription` including `realtime_stream`, `speech_translation` (`realtime` feature), `realtime`, `files`, `skills`, batch/{api_types,results}, `tools`, `error`, `stream_util`, and feature-gated `realtime_ws`. No file exceeds 800 lines.
- [Decision] Provider IDs are `<name>.<family>` for responses/chat/completion/embedding/image/speech/transcription/speech-translation/realtime/batch/files/skills. Custom names change prefixes. Configure provider-options key separately from file reference key (`config.name`); absent custom options fall back to `openai`. Compatible endpoints need distinct identity while accepting standard option keys.
- [Decision] Three language streams share `drive_stream`, promoted from OpenAI `stream_util` to provider utilities during Anthropic implementation. `stream_util` retains re-exports/error/format helpers. `StreamMachine` handle/finish runs through unfold; the driver tracks open parts, closes them before `Error`, then terminates without `Finish`. Core and contract checkers treat `Error` as terminal, so a later `Finish` would never be consumed.
- [Decision] `fail_on_early_error` reads initial chunks before returning the stream. Errors before output (Responses `error`/`response.failed` or Chat/Completion `error` objects) fail `do_stream` as `ApiCall` with inferred status, enabling request retries. After `response.in_progress`, wait at most another 50 ms for output; replay consumed chunks unchanged.
- [Fact] WebSocket transcription/translation authenticate through `realtime` and openai-insecure-api-key subprotocols, stripping Authorization. Derive `ws`/`wss` from `base_url`. Without `realtime`, transcription supports_stream is `false` and speech translation returns an explanatory `NoSuchModelError`.
- [Fact] Tests live under tests/suite, aggregated by `tests/all.rs`; area fixtures use `-stream` suffixes. Local `tokio-tungstenite` servers echo subprotocols and record messages. Fixtures are handwritten (PV-031).
- [Fact] Four additional Responses cases were recorded through a third-party proxy on 2026-09-17 and pass replay comparisons; see the [OpenAI guide](../providers/openai.md#recorded-proxy-verification-2026-09-17). This does not close PV-031 for original fixtures or official endpoints.
- [Fact] record-fixture was implemented 2026-09-14 ([Workspace layout](../03-engineering/02-workspace-layout.md), section 6); these fixtures have not been rerecorded (PV-031).

## 10. Implementation record (2026-09-13, ferrin-anthropic)

- [Fact] Modules: `config` (shared `config`/credentials/URLs/headers/betas), `capabilities` (token limits/structured/adaptive thinking/sampling/`xhigh`), `options` (model/part/system/tool/reasoning schemas), `api_types`, `error`, `cache_control`, `json_schema`, `usage`, convert_prompt/{mod,user,assistant,provider_results}, `prepare_tools`, request/{mod,validate,body}, output/{mod,results,metadata}, `stream`, `messages`, `tools`, `files`, `skills`, batch/{mod,results}, `path`. Largest file: 613 lines.
- [Decision] IDs use `<name>.messages/batch/files/skills`; chat aliases `messages`. Read `anthropic` options first, then custom `name` overrides; write result metadata to both keys when distinct. File/skill references use `name`, preserving standard-key compatibility for custom instances.
- [Decision] Config flags `supports_strict_tools` and `supports_native_structured_output` default `true`. Disable `strict` tools with a warning or force JSON-tool output fallback when compatible hosted endpoints lack those capabilities.
- [Decision] `AnthropicStreamState` implements shared `StreamMachine` using block indexes as IDs. Prefilled `tool_use` blocks in `message_start` emit complete tool input/calls; block stop completes empty input as `{}`. Initial `error` events become `ApiCall` for request retries; later errors terminate as `StreamPart::Error`.
- [Decision] JSON fallback uses tool name `json`, description `Respond with a JSON object.`, and choice any with parallel tools disabled, without the structured-output beta. Batches reject fallback because result conversion lacks request-time mappings to distinguish the synthetic tool.
- [Fact] Merge configured/call/inferred `anthropic-beta` values, including `anthropicBeta` options and batch options, by lowercasing, deduplicating, sorting, and comma joining. Capability beta flags are in the provider guide.
- [Fact] `max_tokens` defaults to the model limit; budget thinking adds `budget_tokens` then clips to the limit, warning only for explicit `maxOutputTokens`. Unknown `claude-*` IDs use latest capabilities and warn about `maxOutputTokens` compatibility.
- [Fact] Fifty-eight suite tests; area fixtures encode event/data lines using encode_events_file. Fixtures remain handwritten (PV-031).
- [Fact] record-fixture was implemented 2026-09-14 ([Workspace layout](../03-engineering/02-workspace-layout.md), section 6); these fixtures have not been rerecorded (PV-031).

## 11. Implementation record (2026-09-13, ferrin-openai-compatible)

- [Fact] Modules: `config` (URLs/query/headers/Bearer/hooks), `options_key` (camel-case conversion, merging/passthrough/`metadata`/deprecation/shared extras), `error` (`ErrorStructure`/default handlers/status inference), `metadata` (extractor traits/merge helpers), chat/{mod,api_types,options,prepare_tools,convert_prompt,output,stream}, `completion`, `embedding`, `image`. Largest file: 554 lines.
- [Decision] Families are chat/completion/embedding/image only; no Responses ([ADR 0014](../04-decisions/2026-09-13-0014-openai-compatible-model-families.md)). `Provider::language_model` returns Chat.
- [Decision] Merge option keys in order: `deprecated` `openai-compatible`, `openaiCompatible`, name, camelCase(name), later wins. Metadata uses the caller's camelCase key when used, otherwise name. Non-camelCase named options warn `deprecated`. Camel conversion replaces only underscore/hyphen followed by ASCII lowercase. Accept both forms for configuration interoperability.
- [Decision] Only unconsumed fields under named/camelCase keys pass through to Chat/Completion/Image request bodies, never shared `openaiCompatible` extras. Message/part options read only the shared key and spread into wire objects, separating common from endpoint-specific options.
- [Decision] Read `api_key_env` per request; missing values omit Authorization without error, supporting unauthenticated local endpoints.
- [Decision] `ChatStreamState` uses the shared driver, text/reasoning IDs `txt-0`/`reasoning-0`, and `index`-buffered tool deltas until names arrive. Early `error` frames become ApiCall with heuristic status; later frames terminate as Error. Missing `finish_reason` emits `InvalidResponseData` rather than `Finish` with an `error` reason, preserving an explanation for truncated streams.
- [Decision] Image edits accept byte files/masks only, matching OpenAI; use `image` for one file and `image[]` for multiple. Multipart needs bytes; secure downloads belong to the core.
- [Decision] `ErrorStructure` applies to all four models; `MetadataExtractor`, `transform_request_body`, and `convert_usage` apply only to Chat, supporting specialized adapters.
- [Fact] Forty-three suite tests; fixtures contain data-only events ending in `[DONE]`, directly replayed by Fixture. Fixtures remain handwritten (PV-031).
- [Fact] record-fixture was implemented 2026-09-14 ([Workspace layout](../03-engineering/02-workspace-layout.md), section 6); these fixtures have not been rerecorded (PV-031).

## 12. Implementation record (2026-09-14, ferrin-google)

- [Fact] Modules: `config` (model/action/origin/WebSocket URLs, authenticated/unauthenticated headers), `api_types` (`generateContent`, `RpcStatus`, numeric/string counts), `capabilities` (Gemini 2/2.5/3 and thinking limits), `options`, `json_schema` (OpenAPI subset), `json_accumulator` (`partialArgs` deltas), `convert_prompt`, `prepare_tools`, `request`, `output`, `stream`, `language_model`, `embedding`, `image`, `speech`, `transcription`, `video`, `files`, batch/{mod,results}, `realtime`, `tools`, `error`. Largest file: 656 lines.
- [Decision] IDs use generative-ai/speech/transcription/batch/realtime suffixes; embedding/image/video/files use bare name. Family identity aids telemetry and error diagnosis.
- [Decision] Read `google` then custom-`name` options, and write metadata to both distinct keys, as with Anthropic.
- [Decision] Images reuse language models: prepare_call converts `ImageOptions` to `CallOptions` with IMAGE response modality, `imageConfig`, and `googleSearch`; `image_result` extracts file parts. Batch images use the same path because Gemini has no separate image endpoint.
- [Decision] `GoogleStreamState` uses the shared driver, increasing integer text/reasoning IDs, and `JsonAccumulator` for `partialArgs`. Keep `willContinue` strings open until the next segment or finalization. Gemini errors use HTTP, so no early-error probing; blocked prompts finish `content-filter` and retain `blockReason`.
- [Decision] For Gemini 3+ replay, if no function call in an assistant message has `thoughtSignature`, add `skip_thought_signature_validator` to all calls and emit one `other` warning. If any signature exists, leave the message untouched. The documented sentinel avoids HTTP 400 for application-lost signatures.
- [Decision] Files use `filename` as `displayName` unless explicitly supplied, without warning; upload/metadata/delete are supported, download is not. Poll uploaded files to `ACTIVE` using configured interval/timeout. Secure downloading belongs to core.
- [Decision] Batches use display name `ferrin-batch-<id>`. At inline size ≥20 MB, upload JSONL through `inputConfig.fileName`; only that path records `inputFileId`/`inputFileExpiresAt`. Require one model per batch; image masks or n >1 return `InvalidArgument`. The endpoint embeds the model; errors match image preparation. JSONL upload lacks fixture coverage; see the Google guide.
- [Decision] Video implements only `do_start`/`do_status`; `do_generate` is unsupported because Veo uses `predictLongRunning`. Realtime token requests authenticate with query `key`, without `x-goog-api-key`, matching that endpoint.
- [Decision] Exclude Interactions beyond transcription, Live streaming transcription, speech translation, and `downloadToolResultFiles`; see the Google guide.
- [Fact] Seventy-eight suite tests; data-event fixtures use encode_events_file with doubled backslashes for JSON escapes. Fixtures remain handwritten (PV-031).

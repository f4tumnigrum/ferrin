# Middleware and registry

**English** | [Chinese](../zh-CN/01-architecture/10-middleware-and-registry.md)

Implemented in `ferrin-core::middleware` and `ferrin-core::registry`.

## 1. Language model middleware

### 1.1 Interface

[Decision] `LanguageModelMiddleware` has six optional hooks: override provider ID, model ID, or supported URLs; transform call parameters (distinguishing generation/streaming); wrap `do_generate`; and wrap `do_stream`. The first three maintain wrapper identity/capabilities; the others cover input and output transformations.

```rust
pub trait LanguageModelMiddleware: Send + Sync + 'static {
    fn override_provider(&self, model: &dyn DynLanguageModel) -> Option<ProviderId> { None }
    fn override_model_id(&self, model: &dyn DynLanguageModel) -> Option<ModelId> { None }
    fn override_supported_urls(&self, model: &dyn DynLanguageModel) -> Option<BoxFuture<'static, SupportedUrls>> { None }

    fn transform_params(
        &self,
        params: CallOptions,
        call_type: CallType,                          // Generate | Stream
        model: &dyn DynLanguageModel,
    ) -> BoxFuture<'_, Result<CallOptions, Error>> { Box::pin(async move { Ok(params) }) }

    fn wrap_generate(
        &self,
        next: ModelCall<'_>,                          // { do_generate(), do_stream(), params, model }
    ) -> BoxFuture<'_, Result<GenerateResult, ProviderError>> { next.do_generate() }

    fn wrap_stream(
        &self,
        next: ModelCall<'_>,
    ) -> BoxFuture<'_, Result<StreamResult, ProviderError>> { next.do_stream() }
}
```

`ModelCall` exposes both `do_generate` and `do_stream`, letting `wrap_stream` call non-streaming generation, as required by simulated streaming.

### 1.2 Composition order

[Decision] `wrap_language_model(model, [a, b, c])` reverses the array and wraps successively: `a` transforms input first, and `c` is closest to the model. Optional `model_id`/`provider_id` override wrapper identity. Outer-to-inner composition follows written order.

```rust
pub fn wrap_language_model(
    model: impl Into<LanguageModelRef>,
    middleware: impl IntoIterator<Item = Arc<dyn LanguageModelMiddleware>>,
) -> LanguageModelRef;

pub struct WrapOptions { pub model_id: Option<ModelId>, pub provider_id: Option<ProviderId> }
pub fn wrap_language_model_with(model: impl Into<LanguageModelRef>, middleware: ..., options: WrapOptions) -> LanguageModelRef;
```

The result implements `DynLanguageModel`. Resolve `provider()` and `model_id()` by explicit override, then middleware override, then original model.

### 1.3 Built-in middleware

[Decision] Built-ins:

| Middleware | Behavior |
| --- | --- |
| `default_settings` | Deep-merge defaults with call parameters, giving the call precedence. |
| `extract_reasoning` (`tag_name`, `separator` default `\n`, `start_with_reasoning` default false) | Extract `<tag>...</tag>` into reasoning parts. Streaming splits at tag boundaries, prevents `text-start` preceding `reasoning-start`, and emits reasoning start/end even for empty blocks. |
| `simulate_streaming` | Wrap `do_stream` by calling `do_generate`, then emit `stream-start`, `response-metadata`, per-part start/delta/end, and `finish`. |
| `extract_json` | Extract a JSON code block from text as the response. |
| `add_tool_input_examples` | Append `input_examples` to tool descriptions for providers without example fields. |

Implementations in `ferrin_core::middleware::builtin`: `default_settings`, `extract_reasoning`, `simulate_streaming`, `extract_json`, and `add_tool_input_examples`.

### 1.4 Image middleware

[Decision] `ImageModelMiddleware` and `wrap_image_model` transform parameters and wrap `do_generate`; `wrap_provider` applies middleware to all of a provider's language and image models.

Ferrin provides these interfaces and `wrap_provider(provider, ProviderMiddleware { language_model: Vec<_>, image_model: Vec<_> })`.

## 2. Registry

### 2.1 ProviderRegistry

[Decision] Provider registries default to separator `:` and may attach language/image middleware:

- `language_model("openai:gpt-5")` splits at the first separator. Missing separators return `NoSuchModel`; unknown providers return `NoSuchProvider` with the requested ID and available list; missing models return `NoSuchModel`.
- Apply registry middleware to every resolved language/image model.
- Support `embedding_model`, `image_model`, `transcription_model`, `speech_model`, `reranking_model`, `video_model`, `files(provider_id)`, and `skills(provider_id)`.

```rust
pub struct ProviderRegistry { /* ... */ }

impl ProviderRegistry {
    pub fn builder() -> ProviderRegistryBuilder;
    pub fn language_model(&self, id: &str) -> Result<LanguageModelRef, Error>;
    pub fn embedding_model(&self, id: &str) -> Result<EmbeddingModelRef, Error>;
    pub fn image_model(&self, id: &str) -> Result<ImageModelRef, Error>;
    pub fn transcription_model(&self, id: &str) -> Result<TranscriptionModelRef, Error>;
    pub fn speech_model(&self, id: &str) -> Result<SpeechModelRef, Error>;
    pub fn reranking_model(&self, id: &str) -> Result<RerankingModelRef, Error>;
    pub fn video_model(&self, id: &str) -> Result<VideoModelRef, Error>;
    pub fn files(&self, provider_id: &str) -> Result<FilesRef, Error>;
    pub fn skills(&self, provider_id: &str) -> Result<SkillsRef, Error>;
}

impl ProviderRegistryBuilder {
    pub fn provider(self, id: impl Into<ProviderId>, provider: Arc<dyn Provider>) -> Self;
    pub fn separator(self, sep: impl Into<String>) -> Self;
    pub fn language_model_middleware(self, mw: Vec<Arc<dyn LanguageModelMiddleware>>) -> Self;
    pub fn image_model_middleware(self, mw: Vec<Arc<dyn ImageModelMiddleware>>) -> Self;
    pub fn build(self) -> ProviderRegistry;
}
```

`ProviderRegistry` implements `Provider`, allowing nesting.

### 2.2 CustomProvider

[Decision] `custom_provider` maps aliases to configured model instances and file/skill interfaces, delegating misses to an optional fallback provider or returning `NoSuchModel`. Semantic aliases such as `fast` and `smart` let applications switch providers by editing mappings.

```rust
pub fn custom_provider() -> CustomProviderBuilder;

impl CustomProviderBuilder {
    pub fn language_model(self, alias: impl Into<ModelId>, model: impl Into<LanguageModelRef>) -> Self;
    pub fn embedding_model(...) -> Self;
    pub fn image_model(...) -> Self;
    // transcription, speech, reranking, video
    pub fn files(self, files: FilesRef) -> Self;
    pub fn skills(self, skills: SkillsRef) -> Self;
    pub fn fallback(self, provider: Arc<dyn Provider>) -> Self;
    pub fn build(self) -> Arc<dyn Provider>;
}
```

### 2.3 Model references and string resolution

[Fact] Resolving string model IDs through an implicit process-global provider can initiate unconfigured network requests and makes behavior depend on global mutable state.

[Decision] Model parameters use `impl Into<LanguageModelRef>`. String references through `LanguageModelRef::from_id("openai:gpt-5")` require an explicitly installed process-wide default registry:

```rust
pub fn set_default_registry(registry: ProviderRegistry) -> Result<(), DefaultRegistryAlreadySet>;
pub fn default_registry() -> Option<&'static ProviderRegistry>;
```

Without it, string calls return `Error::NoDefaultRegistry`. The [project scope](../00-overview/01-project-scope.md) excludes implicit gateways; one-time `OnceLock` initialization avoids runtime races.

## 3. Example

```rust
let registry = ProviderRegistry::builder()
    .provider("openai", ferrin_openai::create_openai(Default::default())?)
    .provider("anthropic", ferrin_anthropic::create_anthropic(Default::default())?)
    .language_model_middleware(vec![Arc::new(default_settings(CallDefaults { temperature: Some(0.2), ..Default::default() }))])
    .build();

let model = registry.language_model("anthropic:claude-sonnet-4-5")?;
let wrapped = wrap_language_model(model, [Arc::new(extract_reasoning("think")) as Arc<dyn LanguageModelMiddleware>]);
```

## 4. Verification items

- [Decision] (PV-010) Minimum `extract_reasoning` coverage is 14 cases. Five non-streaming cases: extract `<think>`, extraction without body text, multiple tags, prepended reasoning with `start_with_reasoning`, and preserved other properties. Nine streaming cases cover missing IDs without panic, split tags, multiple tags in one chunk, no body text, prepended reasoning, unchanged untagged text, empty `<think></think>`, and corresponding ID/order assertions. These cover error-prone tag/chunk boundary combinations.
- [Decision] `ferrin-core` tests must include these 14 cases with corresponding names in `tests/suite/middleware/extract_reasoning.rs`; implementation PRs require all cases before merge.

## 5. Implementation record (2026-09-13)

- [Fact] In addition to section 1.3, `ferrin_core::middleware::builtin` provides `default_instructions(instructions)`, prepending instructions when no system message exists. `extract_reasoning(tag_name)` configures `separator` and `start_with_reasoning`; `add_tool_input_examples()` configures `prefix`, `format`, and `remove`; `extract_json()` offers `transform`. `default_settings(CallDefaults)` deep-merges `provider_options` with `merge_json_objects`, favoring call parameters.
- [Decision] As a `Provider`, `ProviderRegistry::provider_id()` is `"registry"`, giving nested registries a stable identity for errors and telemetry.
- [Fact] `ProviderRegistry::realtime_model(id)` resolves through provider `realtime()`; unsupported realtime returns `NoSuchModel` with `ModelKind::Realtime`. It also exposes `files(provider_id)`, `skills(provider_id)`, and `speech_translation_model(id)`.
- [Decision] Configure the default registry once with `registry::set_default_registry()` and `OnceLock`. Repeated setup returns `Error::InvalidArgument { argument: "registry" }`; string IDs without a registry return `Error::NoDefaultRegistry`.

[Fact] Streaming reasoning extraction flushes incomplete tag prefixes literally at text end, finish, and end of stream. Every extracted block, including consecutive empty or unclosed blocks, has paired start/end events; reasoning IDs are unique across source text parts (2026-09-15, `tests/suite/middleware/extract_reasoning.rs`).

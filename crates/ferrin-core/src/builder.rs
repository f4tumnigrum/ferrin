//! Builder methods shared by `generate_text`, `stream_text` and agents.

/// Generates the configuration methods for a builder type holding a
/// `config: CallConfig` field.
macro_rules! impl_call_builder {
    ($ty:ident) => {
        $crate::builder::impl_call_builder!(@impl $ty [O]);
    };
    ($ty:ident < $($generic:ident),+ >) => {
        $crate::builder::impl_call_builder!(@impl $ty [$($generic),+]);
    };
    (@impl $ty:ident [$($generic:ident),+]) => {
        impl<$($generic),+> $ty<$($generic),+> {
            // ---- prompt ----

            /// Sets system instructions.
            #[must_use]
            pub fn system(mut self, instructions: impl Into<$crate::prompt::Instructions>) -> Self {
                self.config.system = Some(instructions.into());
                self
            }

            /// Sets a single user prompt (mutually exclusive with `messages`).
            #[must_use]
            pub fn prompt(mut self, text: impl Into<String>) -> Self {
                self.config.prompt = Some(text.into());
                self
            }

            /// Sets the conversation messages (mutually exclusive with `prompt`).
            #[must_use]
            pub fn messages(
                mut self,
                messages: impl IntoIterator<Item = ::ferrin_message::Message>,
            ) -> Self {
                self.config.messages = Some(messages.into_iter().collect());
                self
            }

            /// Allows system messages inside `messages`.
            #[must_use]
            pub fn allow_system_in_messages(mut self) -> Self {
                self.config.allow_system_in_messages = true;
                self
            }

            // ---- sampling ----

            /// Replaces all sampling settings.
            #[must_use]
            pub fn settings(mut self, settings: $crate::prompt::CallSettings) -> Self {
                self.config.settings = settings;
                self
            }

            /// Sets the maximum number of output tokens.
            #[must_use]
            pub fn max_output_tokens(mut self, n: u32) -> Self {
                self.config.settings.max_output_tokens = Some(n);
                self
            }

            /// Sets the sampling temperature.
            #[must_use]
            pub fn temperature(mut self, t: f64) -> Self {
                self.config.settings.temperature = Some(t);
                self
            }

            /// Sets nucleus sampling.
            #[must_use]
            pub fn top_p(mut self, p: f64) -> Self {
                self.config.settings.top_p = Some(p);
                self
            }

            /// Sets top-k sampling.
            #[must_use]
            pub fn top_k(mut self, k: u32) -> Self {
                self.config.settings.top_k = Some(k);
                self
            }

            /// Sets the presence penalty.
            #[must_use]
            pub fn presence_penalty(mut self, p: f64) -> Self {
                self.config.settings.presence_penalty = Some(p);
                self
            }

            /// Sets the frequency penalty.
            #[must_use]
            pub fn frequency_penalty(mut self, p: f64) -> Self {
                self.config.settings.frequency_penalty = Some(p);
                self
            }

            /// Sets stop sequences.
            #[must_use]
            pub fn stop_sequences(
                mut self,
                sequences: impl IntoIterator<Item = impl Into<String>>,
            ) -> Self {
                self.config.settings.stop_sequences =
                    Some(sequences.into_iter().map(Into::into).collect());
                self
            }

            /// Sets the random seed.
            #[must_use]
            pub fn seed(mut self, seed: u64) -> Self {
                self.config.settings.seed = Some(seed);
                self
            }

            /// Sets the reasoning effort.
            #[must_use]
            pub fn reasoning(mut self, effort: ::ferrin_spec::ReasoningEffort) -> Self {
                self.config.settings.reasoning = effort;
                self
            }

            // ---- tools ----

            /// Sets the tool set.
            #[must_use]
            pub fn tools(mut self, tools: ::ferrin_tool::ToolSet) -> Self {
                self.config.tools = tools;
                self
            }

            /// Sets the tool choice.
            #[must_use]
            pub fn tool_choice(mut self, choice: ::ferrin_spec::ToolChoice) -> Self {
                self.config.tool_choice = Some(choice);
                self
            }

            /// Restricts the tools sent to the model.
            #[must_use]
            pub fn active_tools(
                mut self,
                names: impl IntoIterator<Item = impl Into<::ferrin_spec::ToolName>>,
            ) -> Self {
                self.config.active_tools = Some(names.into_iter().map(Into::into).collect());
                self
            }

            /// Sets the order in which tools are sent (listed first, rest
            /// alphabetical).
            #[must_use]
            pub fn tool_order(
                mut self,
                names: impl IntoIterator<Item = impl Into<::ferrin_spec::ToolName>>,
            ) -> Self {
                self.config.tool_order = names.into_iter().map(Into::into).collect();
                self
            }

            /// Sets the shared tools context (validated against each tool's
            /// context schema).
            #[must_use]
            pub fn tools_context(mut self, context: ::ferrin_spec::JsonValue) -> Self {
                self.config.tools_context = Some(context);
                self
            }

            /// Sets application state for step preparation, approval and lifecycle hooks.
            /// This value is separate from tool execution context and is never sent to models.
            #[must_use]
            pub fn runtime_context(mut self, context: ::ferrin_spec::JsonValue) -> Self {
                self.config.runtime_context = Some(context);
                self
            }

            /// Sets the call-level approval policy.
            #[must_use]
            pub fn tool_approval(
                mut self,
                policy: impl $crate::generate_text::ApprovalPolicy + 'static,
            ) -> Self {
                self.config.tool_approval = Some(::std::sync::Arc::new(policy));
                self
            }

            /// Sets the secret used to sign approval requests.
            #[must_use]
            pub fn tool_approval_secret(mut self, secret: ::secrecy::SecretBox<[u8]>) -> Self {
                self.config.tool_approval_secret = Some(::std::sync::Arc::new(secret));
                self
            }

            /// Restricts which callers may trigger each tool.
            #[must_use]
            pub fn tool_callers(mut self, callers: ::ferrin_tool::ToolCallers) -> Self {
                self.config.tool_callers = callers;
                self
            }

            /// Sets the tool call repair function.
            #[must_use]
            pub fn repair_tool_call(
                mut self,
                repair: impl $crate::generate_text::ToolCallRepair + 'static,
            ) -> Self {
                self.config.repair_tool_call = Some(::std::sync::Arc::new(repair));
                self
            }

            /// Rewrites the validated input of `name` before execution.
            #[must_use]
            pub fn refine_tool_input(
                mut self,
                name: impl Into<::ferrin_spec::ToolName>,
                refine: impl Fn(
                    ::ferrin_spec::JsonValue,
                ) -> ::ferrin_spec::BoxFuture<
                    'static,
                    Result<::ferrin_spec::JsonValue, $crate::Error>,
                > + Send
                + Sync
                + 'static,
            ) -> Self {
                self.config
                    .refine_tool_inputs
                    .insert(name, ::std::sync::Arc::new(refine));
                self
            }

            /// Sets the sandbox handed to tools.
            #[cfg(feature = "sandbox")]
            #[must_use]
            pub fn sandbox(
                mut self,
                sandbox: ::std::sync::Arc<dyn ::ferrin_tool::Sandbox>,
            ) -> Self {
                self.config.sandbox = Some(sandbox);
                self
            }

            /// Limits how many tools execute concurrently within a step.
            #[must_use]
            pub fn max_tool_concurrency(mut self, n: usize) -> Self {
                self.config.max_tool_concurrency = Some(n.max(1));
                self
            }

            // ---- loop control ----

            /// Adds a stop condition (any-of). Without conditions the loop
            /// stops after one step.
            #[must_use]
            pub fn stop_when(
                mut self,
                condition: impl $crate::generate_text::StopCondition + 'static,
            ) -> Self {
                self.config
                    .stop_conditions
                    .push(::std::sync::Arc::new(condition));
                self
            }

            /// Sets the per-step preparation callback.
            #[must_use]
            pub fn prepare_step(
                mut self,
                prepare: impl $crate::generate_text::PrepareStep + 'static,
            ) -> Self {
                self.config.prepare_step = Some(::std::sync::Arc::new(prepare));
                self
            }

            // ---- request ----

            /// Sets the maximum number of retries (exponential backoff).
            #[must_use]
            pub fn max_retries(mut self, n: u32) -> Self {
                self.config.retry_policy.max_retries = n;
                self
            }

            /// Replaces the retry policy.
            #[must_use]
            pub fn retry_policy(mut self, policy: $crate::retry::RetryPolicy) -> Self {
                self.config.retry_policy = policy;
                self
            }

            /// Sets timeouts.
            #[must_use]
            pub fn timeout(mut self, timeout: impl Into<$crate::timeout::Timeout>) -> Self {
                self.config.timeout = timeout.into();
                self
            }

            /// Sets the cancellation token.
            #[must_use]
            pub fn cancellation(mut self, token: ::tokio_util::sync::CancellationToken) -> Self {
                self.config.cancellation = token;
                self
            }

            /// Adds request headers.
            #[must_use]
            pub fn headers(mut self, headers: ::ferrin_spec::Headers) -> Self {
                self.config.settings.headers.merge(&headers);
                self
            }

            /// Adds provider options.
            #[must_use]
            pub fn provider_options(mut self, options: ::ferrin_spec::ProviderOptions) -> Self {
                for (key, value) in options {
                    self.config
                        .settings
                        .provider_options
                        .entry(key)
                        .or_default()
                        .extend(value);
                }
                self
            }

            /// Sets the download function for file URLs.
            #[must_use]
            pub fn download(
                mut self,
                download: ::std::sync::Arc<dyn $crate::prompt::DownloadFn>,
            ) -> Self {
                self.config.download = Some(download);
                self
            }

            /// Controls which payloads are kept in step results.
            #[must_use]
            pub fn include(mut self, include: $crate::generate_text::Include) -> Self {
                self.config.include = include;
                self
            }

            /// Sets the clock used for `response.timestamp` when the provider sends none.
            #[must_use]
            pub fn clock(mut self, clock: ::std::sync::Arc<dyn $crate::clock::Clock>) -> Self {
                self.config.clock = clock;
                self
            }

            /// Sets the id generator.
            #[must_use]
            pub fn id_generator(
                mut self,
                generator: ::std::sync::Arc<dyn ::ferrin_provider_util::IdGenerator>,
            ) -> Self {
                self.config.id_generator = generator;
                self
            }

            // ---- observability ----

            /// Sets telemetry options.
            #[must_use]
            pub fn telemetry(mut self, options: $crate::telemetry::TelemetryOptions) -> Self {
                self.config.telemetry = options;
                self
            }

            /// Adds all hooks of `hooks`.
            #[must_use]
            pub fn hooks(mut self, hooks: $crate::hooks::Hooks) -> Self {
                self.config.hooks = ::std::mem::take(&mut self.config.hooks).merged(hooks);
                self
            }

            /// Runs when the call starts.
            #[must_use]
            pub fn on_start(
                mut self,
                f: impl $crate::hooks::HookFn<$crate::telemetry::StartEvent>,
            ) -> Self {
                self.config.hooks.on_start.push(::std::sync::Arc::new(f));
                self
            }

            /// Runs when a step starts.
            #[must_use]
            pub fn on_step_start(
                mut self,
                f: impl $crate::hooks::HookFn<$crate::telemetry::StepStartEvent>,
            ) -> Self {
                self.config
                    .hooks
                    .on_step_start
                    .push(::std::sync::Arc::new(f));
                self
            }

            /// Runs before each model call.
            #[must_use]
            pub fn on_language_model_call_start(
                mut self,
                f: impl $crate::hooks::HookFn<$crate::telemetry::ModelCallStartEvent>,
            ) -> Self {
                self.config
                    .hooks
                    .on_language_model_call_start
                    .push(::std::sync::Arc::new(f));
                self
            }

            /// Runs after each model call.
            #[must_use]
            pub fn on_language_model_call_end(
                mut self,
                f: impl $crate::hooks::HookFn<$crate::telemetry::ModelCallEndEvent>,
            ) -> Self {
                self.config
                    .hooks
                    .on_language_model_call_end
                    .push(::std::sync::Arc::new(f));
                self
            }

            /// Runs before each tool execution.
            #[must_use]
            pub fn on_tool_execution_start(
                mut self,
                f: impl $crate::hooks::HookFn<$crate::telemetry::ToolExecutionStartEvent>,
            ) -> Self {
                self.config
                    .hooks
                    .on_tool_execution_start
                    .push(::std::sync::Arc::new(f));
                self
            }

            /// Runs after each tool execution.
            #[must_use]
            pub fn on_tool_execution_end(
                mut self,
                f: impl $crate::hooks::HookFn<$crate::telemetry::ToolExecutionEndEvent>,
            ) -> Self {
                self.config
                    .hooks
                    .on_tool_execution_end
                    .push(::std::sync::Arc::new(f));
                self
            }

            /// Runs when a step finishes.
            #[must_use]
            pub fn on_step_end(
                mut self,
                f: impl $crate::hooks::HookFn<$crate::generate_text::StepResult>,
            ) -> Self {
                self.config.hooks.on_step_end.push(::std::sync::Arc::new(f));
                self
            }

            /// Runs when the call finishes.
            #[must_use]
            pub fn on_end(
                mut self,
                f: impl $crate::hooks::HookFn<$crate::telemetry::EndEvent>,
            ) -> Self {
                self.config.hooks.on_end.push(::std::sync::Arc::new(f));
                self
            }
        }
    };
}

pub(crate) use impl_call_builder;

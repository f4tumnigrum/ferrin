# Changelog

All notable changes to this crate are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [Unreleased]

## [0.1.0] - 2026-09-14

### Added

- `Tool` with `ToolKind::{Function, Dynamic, ProviderDefined, ProviderExecuted}`,
  `Description` (static or per-call), `NeedsApproval`, input hooks,
  `to_model_output`, caller definitions, and `ToolBuilder` entry points
  `Tool::function::<I>()`, `function_with_schema`, `dynamic`,
  `provider_defined`, `provider_executed`.
- Execution contract: `ToolExecute`, `ToolOutput::{Preliminary, Final}`,
  `ToolOutputStream`, `ToolContext`, `execute_to_completion`, closure
  adapters `execute` / `execute_stream` / `execute_with`.
- `ToolError` (`Message`, `Json`, `Timeout`, `Cancelled`) and
  `DuplicateToolError`.
- `ToolSet` (insertion-ordered, unique names) with `filter_active`, `merge`,
  `ordered`, `replace`, `remove`.
- `model_output::{create_tool_model_output, tool_error_output, error_message}`
  and `ErrorMode`.
- `callers`: `ToolCaller`, `ToolCallers`, `ToolCallerDefinition`,
  `validate_tool_callers`, `prepare_tools_for_callers`.
- `fingerprint`: `canonical_json`, `hash_canonical`, `fingerprint_tools`,
  `detect_tool_drift`, `ToolDrift`.
- `DescriptionContext::with_tool_context` for resolving per-call descriptions
  outside the generation loop.
- Feature `sandbox`: `Sandbox`, `SandboxProcess`, option types, and
  `LocalProcessSandbox` (no isolation; tests and examples only).

### Changed

- Licensed under Apache-2.0 only (previously MIT OR Apache-2.0); `LICENSE` and
  `NOTICE` are included in the package (ADR 0017).
- Crate and module documentation attribute the code derived from the Vercel
  AI SDK.

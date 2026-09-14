# 编码规范

[English](../../03-engineering/03-coding-standards.md) | **简体中文**

本规范适用于工作区全部 crate。多数规则由 `rustfmt`、Clippy 与 CI 强制；无法自动检查的规则在代码评审中执行。

## 1. 格式

【决策】`rustfmt.toml` 设置 `edition = "2024"`、`imports_granularity = "Item"`；该选项仅在 nightly rustfmt 中生效，CI 与 `just fmt-check` 以 `cargo fmt -- --config imports_granularity=Item --check` 显式传入。依据：逐项导入使 diff 只涉及被改动的条目，合并冲突更少。

```toml
# rustfmt.toml
edition = "2024"
imports_granularity = "Item"   # nightly-only option; stable rustfmt warns and ignores, CI passes it explicitly
```

- 每个 `use` 一行一个条目；不使用 glob 导入（`prelude` 模块内除外）。
- 行宽 100（rustfmt 默认）。

## 2. Lint 配置

【决策】工作区 Clippy 配置（根 `Cargo.toml` 的 `[workspace.lints.clippy]`）把以下规则设为 `deny`：`await_holding_invalid_type`、`await_holding_lock`、`disallowed_methods`、`expect_used`、`identity_op`、`manual_*` 系列、`needless_*` 系列、`redundant_clone`、`redundant_closure`、`redundant_closure_for_method_calls`、`redundant_static_lifetimes`、`trivially_copy_pass_by_ref`、`uninlined_format_args`、`unnecessary_*` 系列、`unwrap_used`；`clippy.toml` 允许测试中的 `unwrap`/`expect`，把 `tokio::sync::MutexGuard` 等列入 `await-holding-invalid-types`，`large-error-threshold = 256`。依据：这些规则分别防止持锁跨 `.await`（死锁）、库代码 panic、冗余的克隆与闭包；统一在工作区级声明使所有 crate 一致。

【决策】除上述 Clippy 规则外，追加 Rust 编译器 lint：

```toml
[workspace.lints.rust]
unsafe_code = "forbid"
missing_docs = "warn"                 # "deny" via RUSTFLAGS in CI for published crates
unreachable_pub = "warn"
rust_2018_idioms = { level = "warn", priority = -1 }
unused_qualifications = "warn"
missing_debug_implementations = "warn"
unexpected_cfgs = { level = "warn", check-cfg = ["cfg(docsrs)"] }   # `just doc` passes --cfg docsrs

[workspace.lints.clippy]
# base set (all "deny"), listed above
# Ferrin additions
dbg_macro = "deny"
print_stdout = "deny"
print_stderr = "deny"
todo = "deny"
unimplemented = "deny"
large_futures = "warn"
```

```toml
# clippy.toml
allow-expect-in-tests = true
allow-unwrap-in-tests = true
await-holding-invalid-types = [
    "tokio::sync::MutexGuard",
    "tokio::sync::RwLockReadGuard",
    "tokio::sync::RwLockWriteGuard",
]
large-error-threshold = 128
disallowed-methods = [
    { path = "reqwest::Client::get", reason = "Route all HTTP through ferrin_provider_util::http." },
    { path = "reqwest::Client::post", reason = "Route all HTTP through ferrin_provider_util::http." },
    { path = "reqwest::Client::request", reason = "Route all HTTP through ferrin_provider_util::http." },
    { path = "reqwest::Client::execute", reason = "Route all HTTP through ferrin_provider_util::http." },
    { path = "reqwest::get", reason = "Route all HTTP through ferrin_provider_util::http." },
    { path = "std::env::var", reason = "Use ferrin_provider_util::settings for environment lookups." },
    { path = "tokio::spawn", reason = "Use JoinSet so tasks are cancelled with their owner." },
]
```

`ferrin-provider-util` 的 `http` 与 `settings` 模块通过 `#[allow(clippy::disallowed_methods)]` 局部放行并附注释。`xtask` 与 `examples/*` 是命令行程序，在 crate 根以 `#![allow(clippy::print_stdout, clippy::print_stderr)]` 放行并注明原因。

`unsafe_code = "forbid"` 适用于全部 crate；若未来出现必要的 `unsafe`（如 FFI），需 ADR 并在对应 crate 单独降级为 `deny` + 逐处 `#[allow]` 加 `// SAFETY:` 注释。

## 3. 命名与 API 形态

【决策】基础约定：`format!` 内联参数；折叠嵌套 `if`；方法引用优于闭包；避免布尔或裸 `Option` 参数；位置字面量参数使用 `/*param_name*/` 注释；`match` 尽量穷尽、避免通配分支；新增 trait 必须有文档注释说明角色与实现期望；trait 方法使用 `impl Future + Send` 而非 `#[async_trait]`。依据：前几条由 Clippy（`uninlined_format_args`、`collapsible_if`、`redundant_closure_for_method_calls`）机械强制，其余由评审执行，目标是让调用点自说明、让 trait 契约可读。

补充约定：

- 公共枚举 `#[non_exhaustive]`；crate 内部对这些枚举的 `match` 仍要求穷尽（编译器允许 crate 内穷尽匹配），下游必须有通配分支。
- 构造函数命名：`new` 用于无失败的简单构造；可失败的构造用 `try_new` 或 `parse`；构建器用 `builder()`。
- 转换：`From`/`Into` 用于无损转换；`TryFrom` 用于可失败转换；不实现 `Deref` 模拟继承。
- 类型状态：构建器的泛型参数只用于承载输出类型（`GenerateText<O>`），不用类型状态机表达“必填字段”。

## 4. 异步

- trait 方法形态：`fn name(&self, ...) -> impl Future<Output = T> + Send;`，实现可用 `async fn`。
- 对象安全需要时提供 `Dyn*` trait（见 [Provider 规范层](../01-architecture/04-provider-spec.md)）。
- 不在库代码中使用 `block_on`。
- 任务通过 `JoinSet` 管理；`tokio::spawn` 被 lint 禁止。
- 【决策】（2026-09-14 修订，[ADR 0016](../04-decisions/2026-09-14-0016-inline-encoding-no-spawn-blocking.md)；原为“长耗时同步工作使用 `spawn_blocking`”）编码与序列化在异步任务内直接执行，库代码不使用 `spawn_blocking`/`block_in_place`；出现可测量的阻塞工作时以新的 ADR 引入阻塞路径。
- 使用 `#[tracing::instrument(skip_all, fields(...))]` 在函数定义处埋点，不在调用处 `.instrument()`；`skip_all` 防止参数（可能含密钥或完整请求体）被自动记录。

## 5. 错误处理

- 库 crate 不使用 `anyhow`、`eyre`；错误类型用 `thiserror` 定义。
- 不使用 `unwrap`/`expect`（lint 禁止）；不可达分支用 `unreachable!()` 并注释原因。
- 错误消息小写开头、无句号；不含敏感信息。
- `?` 转换依赖 `From` 实现；不在热路径上构造带堆分配的错误（如 `Box<dyn Error>`）除非确实发生错误。

## 6. 模块规模与变更规模

【决策】模块目标 500 行以内（不含测试），超过约 800 行时新功能放入新模块；单次变更不超过 800 行（复杂逻辑 500 行），更大变更拆分为可评审阶段。依据：这是评审者在一次阅读中能够保持上下文的大致规模，且阈值可由 `xtask check-module-size` 机械检查。

CI 中的 `xtask check-module-size` 对超过 800 行的非测试文件发出警告（不阻断），评审时需说明理由。

## 7. 依赖使用

- 新增外部依赖需在 PR 描述中说明用途、替代方案与维护状态；`cargo deny` 通过后方可合并。
- 不使用 `git` 依赖发布 crate；`[patch.crates-io]` 仅用于本地调试且不得提交。
- 不使用 `*` 版本。
- feature 只做增量（见 [Crate 划分与职责](../01-architecture/02-crates.md)第 5 节）。

## 8. 日志

- 使用 `tracing`，不使用 `log` 宏、`println!`/`eprintln!`（lint 禁止）。
- 级别：`error` 仅用于库无法继续的内部不一致；`warn` 用于供应商警告与降级；`info` 用于调用生命周期（默认不含内容）；`debug` 用于请求/响应元数据；`trace` 用于分片级别。
- 不记录密钥、完整请求体或响应体；内容记录仅在 `record_inputs`/`record_outputs` 下以专用 target 输出。

## 9. 文档注释

- 每个公共项：一句话概述、必要的段落、`# Errors`（返回 `Result` 时）、`# Panics`（若可能）、`# Examples`（入口函数必须）。
- trait 文档说明“何时实现”与“实现者义务”。
- 模块级文档（`//!`）说明模块职责与不变量。

## 10. 测试代码

- 测试比较整个对象而非逐字段，使用 `pretty_assertions::assert_eq!`，失败时输出整体 diff。
- 不为静态常量写测试；不为已删除逻辑写否定测试。
- 测试中允许 `unwrap`/`expect`。
- 不在实现文件中放置仅测试使用的辅助函数；辅助放在 `ferrin-testing` 或 `tests/suite/common.rs`。
- 测试文件与源码文件分离：测试放在 `tests/suite/`（见[工作区布局](02-workspace-layout.md)第 4 节），源码文件不含测试模块。

## 11. 提交与 PR

- Conventional Commits 格式：`feat(core): ...`、`fix(openai): ...`、`docs: ...`、`refactor(spec): ...`；scope 为 crate 短名。
- PR 描述包含：动机、变更摘要、测试方式、是否破坏性、关联 ADR/issue。
- 破坏性变更在提交脚注写 `BREAKING CHANGE:`，并更新对应 crate 的 `CHANGELOG.md`。

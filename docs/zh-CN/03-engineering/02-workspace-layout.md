# 工作区布局

[English](../../03-engineering/02-workspace-layout.md) | **简体中文**

## 1. 目录结构

```
ferrin/
  Cargo.toml                 # workspace 根：members、workspace.package、workspace.dependencies、workspace.lints、profiles
  Cargo.lock                 # 提交
  rust-toolchain.toml
  rustfmt.toml
  clippy.toml
  deny.toml
  typos.toml
  cliff.toml                 # git-cliff 变更日志配置
  justfile
  .cargo/config.toml         # `cargo xtask` 别名
  .config/nextest.toml
  .github/workflows/         # ci、typos、semver、coverage、live-tests、versions、release、bench
  .github/dependabot.yml
  .github/pull_request_template.md
  scripts/docs_lint.py       # 文档链接与【待验证】编号检查（CI docs-lint 作业）
  verification/              # 待验证事项原型（独立 workspace，不发布）
  crates/
    ferrin-spec/
    ferrin-schema/
    ferrin-message/
    ferrin-provider-util/
    ferrin-tool/
    ferrin-core/
    ferrin-mcp/
    ferrin-otel/
    ferrin-testing/
    ferrin-macros/
    ferrin/
    providers/
      ferrin-openai/
      ferrin-anthropic/
      ferrin-openai-compatible/
      ferrin-google/
  xtask/
  examples/                  # 独立示例 crate（不发布），每个示例一个二进制
  docs/                      # 本文档集
    00-overview/ 01-architecture/ 02-api/ 03-engineering/ 04-decisions/ 05-appendix/
    README.md                # 设计文档索引与文档约定
    api/                     # `cargo xtask api-snapshot` 生成的公共 API 摘要（每个 crate 一个 JSON，CI 校验）
    providers/               # 每个供应商的能力矩阵、选项与行为说明
  assets/                    # 仓库 README 使用的图片（横幅）；不进入任何发布包
  CHANGELOG.md               # 工作区级；每个 crate 另有 CHANGELOG.md
  CONTRIBUTING.md SECURITY.md
  AGENTS.md                  # AI 编码代理与贡献者的操作摘要（以 docs/ 为准）
  CLAUDE.md                  # 仅导入 AGENTS.md（Claude Code 读取）
  LICENSE NOTICE             # Apache-2.0 全文与派生代码署名；两者复制到每个发布的 crate 目录
```

【事实】2026-09-13 已按此布局建立骨架：15 个 crate（各含 `Cargo.toml`、`src/lib.rs`、`README.md`、`CHANGELOG.md`）、`xtask`、`examples/example-generate-text`、全部配置文件与工作流。骨架在 1.98.1 上通过 `cargo check/clippy/doc/deny/hack/nextest`。

【决策】crate 位于 `crates/` 与 `crates/providers/` 下而非仓库根目录。依据：平铺在仓库根目录的工作区在 crate 数量增长后目录列表难以浏览；Ferrin crate 数量可控但仍按类别分组，供应商适配器单独一级。

【决策】（[ADR 0017](../04-decisions/2026-09-14-0017-apache-2-license-and-attribution.md)）工作区以 Apache-2.0 单许可发布；根目录的 `LICENSE`（许可证全文）与 `NOTICE`（版权与派生代码署名）复制到每个发布的 crate 目录。依据：`cargo package` 只打包 crate 目录内的文件，而随发布附带许可证与声明是 Apache-2.0 第 4 条的要求；副本内容相同，变更时一并同步。

## 2. 工作区根 `Cargo.toml`

```toml
[workspace]
resolver = "3"
members = ["crates/ferrin", "crates/ferrin-*", "crates/providers/*", "xtask", "examples/*"]
exclude = ["verification"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.98"
license = "Apache-2.0"
repository = "https://github.com/f4tumnigrum/ferrin"
authors = ["Ferrin contributors"]

[workspace.dependencies]
# internal
ferrin-spec = { path = "crates/ferrin-spec", version = "0.1.0" }
ferrin-schema = { path = "crates/ferrin-schema", version = "0.1.0" }
# ... every workspace crate
# external — versions from docs/03-engineering/01-toolchain-and-dependencies.md
tokio = { version = "1.53", default-features = false }
serde = { version = "1.0", features = ["derive"] }
serde_json = { version = "1.0", features = ["preserve_order", "raw_value"] }
# ...

[workspace.lints]
# see docs/03-engineering/03-coding-standards.md

[profile.dev]
debug = "limited"

[profile.release]
lto = "thin"
codegen-units = 4
debug = "line-tables-only"

[profile.ci-test]
inherits = "test"
opt-level = 0
debug = "limited"
```

【决策】profile 设置：`dev.debug = "limited"`，`release.lto = "thin"`、`codegen-units = 4`、`debug = "line-tables-only"`，`ci-test` profile 继承 `test` 并降低体积。依据：`limited` 调试信息缩短开发构建时间且足以回溯；thin LTO 与 4 个代码生成单元在发布构建时间与运行性能之间折中；行表级调试信息使发布构建的 panic 回溯仍带行号。

【决策】`resolver = "3"`（edition 2024 默认）以启用 MSRV 感知的依赖解析。

【事实】Cargo 要求 `members` 通配符匹配到的每个目录都有 `Cargo.toml`，因此不能用 `crates/*`（会匹配 `crates/providers` 目录）；改用 `crates/ferrin` + `crates/ferrin-*`。

【事实】成员 crate 不能用 `default-features = false` 覆盖 `[workspace.dependencies]` 中默认开启 features 的条目（Cargo 报错）；因此 `ferrin-provider-util`、`ferrin-core` 在工作区依赖表中声明 `default-features = false`，由门面 crate `ferrin` 以 `default-features = true` 重新开启。

【决策】外部依赖统一在 `[workspace.dependencies]` 声明，成员 crate 以 `{ workspace = true }` 引用。成员 crate 只能在此基础上追加 `features`，不能单独指定版本。

## 3. 成员 crate `Cargo.toml` 模板

```toml
[package]
name = "ferrin-core"
description = "Ferrin core: text generation loop, streaming pipeline, agents, middleware."
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
repository.workspace = true
readme = "README.md"
keywords = ["ai", "llm", "sdk"]
categories = ["api-bindings", "asynchronous"]

[lib]
name = "ferrin_core"
path = "src/lib.rs"

[lints]
workspace = true

[features]
default = ["video"]
video = []
realtime = ["dep:tokio-tungstenite"]

[dependencies]
ferrin-spec = { workspace = true }
tokio = { workspace = true, features = ["rt", "sync", "time", "macros"] }

[dev-dependencies]
ferrin-testing = { workspace = true }
insta = { workspace = true }

[package.metadata.docs.rs]
all-features = true
rustdoc-args = ["--cfg", "docsrs"]
```

## 4. 源码组织规则

- `src/lib.rs` 只包含模块声明、`pub use` 与 crate 级文档；不含逻辑。
- 模块默认私有；公共 API 通过 `lib.rs` 显式 re-export。
- 测试代码不与源码文件放在同一目录：`src/` 下不出现 `*_tests.rs`，源码文件内也不放 `#[cfg(test)] mod tests`。
- 测试位于 `tests/suite/*.rs`，由 `tests/all.rs` 汇总为一个测试二进制，减少链接时间。`tests/all.rs` 在 crate 根放行 `clippy::unwrap_used`/`clippy::expect_used`（`clippy.toml` 的 `allow-unwrap-in-tests` 只识别 `#[test]` 函数与 `#[cfg(test)]` 模块，不覆盖集成测试中的辅助函数）。
- 只能通过 crate 内部可见性触及的逻辑，在 `src/tests/` 子目录中测试（`lib.rs` 以 `#[cfg(test)] mod tests;` 引入，被测项需为 `pub(crate)`）；优先把逻辑设计为可经公共 API 测试。

【决策】2026-09-13 起不采用与源码同级的 `*_tests.rs` 约定，改为上述布局。依据：测试文件与源码文件分离后，源码目录只含实现，评审与模块规模统计不受测试代码干扰。
- fixture 位于 `tests/fixtures/<area>/<case>.*`。
- 基准测试位于 `benches/<name>.rs`（清单声明 `[[bench]] harness = false`，criterion），只对热点路径编写：SSE 解码、部分 JSON 修复、schema、消息裁剪、工具指纹、生成与流式管线、供应商适配器、门面端到端；目标清单与运行方式见[测试规范](04-testing.md)第 11 节。

## 5. 示例目录

`examples/` 下每个子目录是独立的二进制 crate（`publish = false`），命名 `example-<topic>`：`example-generate-text`、`example-stream-sse-server`、`example-tool-approval`、`example-mcp`、`example-structured-output`、`example-agent`、`example-otel`。示例代码是文档的一部分，CI 编译全部示例。

- 【事实】（2026-09-14）七个示例均已实现，全部只依赖 `ferrin` 门面（按需开启 `openai`、`mcp`、`otel` feature）、`tokio` 与 `anyhow`，并通过 `ferrin::provider_util::settings::env_var` 读取 `OPENAI_API_KEY`（由 `OpenAiSettings::default()` 延迟读取）、`OPENAI_BASE_URL`（同上）与 `OPENAI_MODEL`（默认 `gpt-5`）；使用工具的三个示例（agent、tool-approval、mcp）另读取 `OPENAI_PROVIDER_OPTIONS`（JSON 供应商选项，如 `{"openai":{"store":false}}`）。`example-mcp` 调用 `server-everything` 的 `get-sum` 工具。2026-09-14 以真实凭据（第三方 OpenAI 兼容端点）运行：七个示例全部成功（工具相关示例需上述 `store: false` 选项）。`example-generate-text`：Responses 模型单步生成，打印文本、结束原因与用量；`example-structured-output`：`Output::<Recipe>::object()` 与 `#[serde(crate = "ferrin::serde")]`/`#[schemars(crate = "ferrin::schemars")]` 派生；`example-tool-approval`：`NeedsApproval::Always` 工具，首轮结束后读取 `last_step().tool_approval_requests()`，在终端询问后以 `MessagesExt::push_approval_response` 追加批准或拒绝并发起第二轮；`example-agent`：`#[ferrin::tool]` 定义两个工具，`ToolLoopAgent::builder(..).instructions(..).tools(..).stop_when(step_count(6)).on_step_end(..)`；`example-mcp`：`TransportConfig::Stdio(StdioConfig::new(..).args(..))` 连接 `@modelcontextprotocol/server-everything`（`MCP_SERVER_COMMAND`/`MCP_SERVER_ARGS` 可覆盖），`client.tools(ToolsOptions::default())` 后交给 `generate_text`；`example-stream-sse-server`：hyper 1.x + `hyper-util` + `http-body-util` 服务器，连接任务由 `JoinSet` 持有，`stream_text(..).split()` 的事件流以 `data: <json>` 帧转发并以 `data: [DONE]` 结束；`example-otel`：自定义 `SpanExporter` 打印 span，`SdkTracerProvider` + `tracing-subscriber` + `tracing-opentelemetry`，`OtelTelemetry::builder().tracer_provider(&provider).without_metrics()`，调用包在 `tracing::info_span!` 内以展示父子关系。示例不在 CI 中运行（需要真实凭据），只编译（`examples` 作业）。

## 6. `xtask`

```
cargo xtask record-fixture --provider <name> --case <case>
cargo xtask check-versions
cargo xtask publish-order          # prints crates in dependency order
cargo xtask api-snapshot           # dumps public API via rustdoc JSON for review
cargo xtask check-module-size      # warns on non-test files above 800 lines
```

别名定义在 `.cargo/config.toml`（`xtask = "run --quiet --package xtask --"`）。

- 【事实】（2026-09-14）五个子命令均已实现，源码按命令拆分为 `xtask/src/{publish_order, module_size, check_versions, record_fixture}.rs` 与 `xtask/src/api_snapshot/{mod, summary, render}.rs`，共享辅助（`cargo metadata`、发布判定、tokio 当前线程运行时）在 `workspace.rs`。`publish-order` 基于 `cargo_metadata` 的 Kahn 拓扑排序（仅计 normal/build 依赖，同层按名称排序）；`check-module-size` 统计各成员 `src/` 下非 `_tests.rs` 文件的行数。
- 【事实】`check-versions`：取工作区成员的直接外部依赖（`cargo metadata` 解析图中来自 crates.io 的包，锁定多个版本时取最高者），经 `ferrin_provider_util::default_transport` 并发（`JoinSet`）读取 crates.io 稀疏索引（`https://index.crates.io/<prefix>/<name>`），取未 yank 的最高非预发布版本比较；向标准输出写 Markdown（过期表格或“全部最新”一行），存在过期依赖或索引查询失败时以非零退出码结束（`versions.yml` 据此开 issue）。2026-09-14 首次运行：52 个直接外部依赖全部为最新稳定版。
- 【决策】`record-fixture --provider <p> --case <area>/<name>` 读取 `crates/providers/ferrin-<p>/tests/fixtures/<area>/<name>.scenario.json`（字段 `method`（默认 `POST`）、`path`（含查询串）、`base_url`、`api_key_env`、`headers`、`body`、`stream`、`model`；未知字段报错），而非[测试规范](04-testing.md)原先写的 `.scenario.rs`/TOML。依据：JSON 由工作区已有的 `serde_json` 解析，无需新增 `toml` 依赖；场景只是一次 HTTP 请求的描述，不需要 Rust 代码。供应商默认值：`openai` 为 `https://api.openai.com/v1` + `OPENAI_API_KEY`（`authorization: Bearer`），`anthropic` 为 `https://api.anthropic.com/v1` + `ANTHROPIC_API_KEY`（`x-api-key`、`anthropic-version: 2023-06-01`），`google` 为 `https://generativelanguage.googleapis.com/v1beta` + `GOOGLE_GENERATIVE_AI_API_KEY`（`x-goog-api-key`）；`openai-compatible` 必须在场景中给出 `base_url` 与 `api_key_env`。密钥经 `ferrin_provider_util::settings::env_var` 读取；请求经 `ferrin_testing::RecordingTransport` 发送，响应头按白名单保留（`content-type`、`x-request-id`/`request-id`、`retry-after`/`retry-after-ms`、`openai-processing-ms`、`openai-version`、`x-ratelimit-*`、`anthropic-ratelimit-*`），响应体上限 64 MiB。写出 `<name>.request.json`（请求体）、`<name>.response.json`（非流式，JSON 重排版）或 `<name>.chunks.txt`（流式，按空行切分 SSE 事件后 `encode_events_file`）与 `<name>.meta.json`（`status`、`headers`、`recorded_at`、`provider`、`case`、`model`）；每个文件写入前以 `ferrin_testing::transport::contains_secret` 与密钥原文检查，命中即失败。`case` 只接受 ASCII 字母、数字、`-`、`_` 与 `/`。
- 【决策】`api-snapshot [--check]` 对 `publish-order` 列出的每个 crate 运行 `RUSTC_BOOTSTRAP=1 cargo rustdoc -p <crate> --lib --all-features -- -Z unstable-options --output-format json`，读取 `target/doc/<crate>.json`，把公共 API 摘要写入 `docs/api/<crate>.json`；`--check` 只比较不写入，存在差异时非零退出（CI `doc` 作业运行）。依据：rustdoc JSON 是唯一机器可读的公共 API 来源，但只在 `-Z unstable-options` 后可用；以 `RUSTC_BOOTSTRAP=1` 在固定的稳定工具链（1.98.1）上启用并校验 `format_version`（当前支持 60），版本不符时命令报错提示更新解析器，避免静默生成错误摘要。摘要从 crate 根模块遍历公共模块，每项记录路径、种类、签名（函数含 `const`/`async`/`unsafe`、泛型与 where 子句；类型别名、常量含类型；trait 含超 trait 与 dyn 兼容性）、`non_exhaustive`/`must_use`/`repr`/`deprecated` 属性、结构体公共字段、枚举变体、trait 项与固有方法、非合成且非 blanket 的 trait 实现，以及 `use` 再导出的来源；不含文档注释与私有项。摘要是评审辅助，不做 semver 判定。
- 【事实】（2026-09-14，首次 CI 运行后修正）rustdoc JSON 中 trait 对象的自动 trait 顺序随目标不同（`aarch64-apple-darwin` 输出 `dyn Error + Sync + Send`，`x86_64-unknown-linux-gnu` 输出 `dyn Error + Send + Sync`），导致本地生成的 `ferrin-spec.json` 在 Linux 上的 `--check` 失败；`render_poly_traits` 现保留首个（主）trait 的位置并对其余 trait 排序，`--check` 失败时逐行打印已提交与新生成摘要的差异（最多 40 行），使 CI 日志足以定位差异。

## 7. `justfile`

```just
set working-directory := "."

fmt:
    cargo fmt --all -- --config imports_granularity=Item

fmt-check:
    cargo fmt --all -- --config imports_granularity=Item --check

clippy *args:
    cargo clippy --workspace --all-targets {{args}} -- -D warnings

fix *args:
    cargo clippy --workspace --all-targets --fix --allow-dirty {{args}}

test *args:
    RUST_MIN_STACK=8388608 NEXTEST_PROFILE=local cargo nextest run --workspace --no-fail-fast {{args}}

doc:
    RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo doc --workspace --no-deps --all-features

deny:
    cargo deny check

shear:
    cargo shear --deny-warnings

features:
    cargo hack check --workspace --each-feature --no-dev-deps

typos:
    typos

check-all: fmt-check clippy test doc deny shear

verify *args:                       # runs the verification/ prototypes
    cargo test --manifest-path verification/Cargo.toml --workspace --no-fail-fast -- --nocapture --test-threads=1 {{args}}

changelog crate:                    # per-crate changelog fragment from Conventional Commits
    git cliff --config cliff.toml --include-path "crates/{{crate}}/**" --unreleased
```

【决策】`just test` 设置 `RUST_MIN_STACK=8388608`、`NEXTEST_PROFILE=local` 并以 `--no-fail-fast` 运行 nextest；`just fix` 为 `cargo clippy --fix --tests --allow-dirty`。依据：深层嵌套的 future 与 `proptest` 用例在默认 2 MiB 线程栈上可能溢出，8 MiB 与 CI 设置一致；`--no-fail-fast` 让单次运行暴露全部失败。

# 测试规范

[English](../../03-engineering/04-testing.md) | **简体中文**

## 1. 测试层次

| 层次 | 位置 | 工具 | 网络 |
| --- | --- | --- | --- |
| 单元测试 | `tests/suite/*.rs`（`tests/all.rs` 汇总；`src/` 下不放测试代码，见第 10 节） | `cargo nextest`、`pretty_assertions`、`proptest` | 否 |
| 集成测试 | `tests/suite/*.rs`（`tests/all.rs` 汇总） | `wiremock`、`ferrin-testing`、`insta` | 仅本地 mock 服务器 |
| fixture 回放测试 | `tests/suite/` + `tests/fixtures/` | `ferrin-testing::FixtureServer` | 否 |
| 契约测试 | `ferrin-testing::StreamContractChecker` | 断言事件顺序 | 否 |
| 文档测试 | rustdoc 示例 | `cargo test --doc` | 否（示例使用 `MockLanguageModel`） |
| 在线测试 | `tests/suite/live_*.rs`，`#[ignore]` | 真实 API，需环境变量密钥 | 是 |
| 基准测试 | `benches/<name>.rs`（`[[bench]] harness = false`；11 个目标见第 11 节） | `criterion` 0.8.2，`just bench [filter]` | 否（mock 模型与本地 fixture 服务器） |
| 编译失败测试 | `crates/ferrin/tests/ui/` | `trybuild` | 否 |

【决策】供应商测试以录制的原始 SSE 分片文件与 JSON 响应作为 fixture，重建响应后对规范化输出做快照断言；在线测试以 `#[ignore]` 门控并要求供应商密钥环境变量。依据：录制的原始分片保留供应商真实的分片边界与字段形状，比手写的规范事件更能暴露解析缺陷；`#[ignore]` 让默认测试运行不依赖网络与密钥。

## 2. 测试执行

- 统一通过 `just test [-p <crate>]` 执行（`cargo nextest run --workspace --no-fail-fast`，`RUST_MIN_STACK=8388608`）。
- `.config/nextest.toml` 定义 `default`、`local`、`ci` profile：`retries = 0`（重试会掩盖不稳定测试）、`slow-timeout = { period = "60s", terminate-after = 1 }`、`fail-fast = false`；`ci` 追加 JUnit 输出（`junit.xml`）与 `failure-output = "immediate-final"`；名称匹配 `test(live_)` 的在线测试超时 300 s。
- 【决策】`trybuild` 编译失败用例放在门面 crate `ferrin` 而非 `ferrin-macros`：用例需要 `ferrin::tool` 路径，若放在 `ferrin-macros` 会形成 `ferrin-macros --dev--> ferrin --> ferrin-macros` 的 dev 依赖环，`cargo publish` 校验时无法解析尚未发布的同版本 `ferrin`。
- 【决策】（2026-09-14，首次 CI 运行后）`tool_macro_ui` 在 Windows 上标记 `#[ignore]`（`cfg_attr(windows, ignore)`）：`windows-2025` 运行器上 trybuild 的冷构建超过 nextest 为其设置的 360 s 上限（run 34797869442，其余 653 个测试全部通过）；被测的是宏诊断文本，与平台无关，由 Linux 与 macOS 作业覆盖。
- 【事实】（2026-09-14）`trybuild` 用例由 `crates/ferrin/tests/suite/ui.rs` 的单个测试驱动（`tests/ui/pass/*.rs`、`tests/ui/fail/*.rs`，`.stderr` 快照随代码提交，`TRYBUILD=overwrite` 刷新）；首次运行需在 `target/tests/trybuild/` 下构建独立项目，因此 `.config/nextest.toml` 为 `test(tool_macro_ui)` 设置 180 s 的慢测试周期（默认 60 s 会在冷缓存下超时）。
- 在线测试：`just test -- --run-ignored only live` 且设置对应环境变量。
- 【事实】（2026-09-14）现有在线测试：`crates/ferrin/tests/suite/live_openai.rs`（门面级：生成、流式、工具往返、结构化输出；环境变量 `OPENAI_API_KEY`，可选 `OPENAI_BASE_URL`、`OPENAI_MODEL`（默认 `gpt-5`）、`OPENAI_PROVIDER_OPTIONS`（JSON 供应商选项））、`crates/providers/ferrin-openai/tests/suite/live_responses.rs`（Responses 与 Chat 族的生成、流式、工具调用产出；同一组变量）、`crates/providers/ferrin-openai-compatible/tests/suite/live_chat.rs`（`OPENAI_COMPATIBLE_BASE_URL`、`OPENAI_COMPATIBLE_API_KEY`、`OPENAI_COMPATIBLE_MODEL`）。当日以一个第三方 OpenAI 兼容端点全部通过；该端点不支持 `item_reference`，需 `OPENAI_PROVIDER_OPTIONS='{"openai":{"store":false}}'`（见 `docs/providers/openai.md`）。

【决策】不允许 nextest 自动重试。依据：SDK 的核心价值是确定性行为，任何不稳定测试都应修复而非重试。

## 3. Fixture 规范

### 3.1 目录与文件

```
crates/providers/ferrin-openai/tests/fixtures/
  responses/
    text-basic.request.json         # recorded request body (secrets stripped)
    text-basic.response.json        # non-streaming response body
    text-basic.chunks.txt           # streaming: one SSE event per line, exactly as received
    text-basic.meta.json            # status code, response headers (allow-listed), recorded_at, model id
    tool-call.chunks.txt
    reasoning.chunks.txt
    error-429.response.json
```

### 3.2 录制

`cargo xtask record-fixture --provider openai --case responses/tool-call` 执行：

1. 从 `tests/fixtures/<case>.scenario.json` 读取场景（方法、路径、请求头、请求体、是否流式、模型 ID）。【决策】（2026-09-14）场景文件为 JSON，而非本节原先写的 `.scenario.rs`/TOML；字段与依据见[工作区布局](02-workspace-layout.md)第 6 节。
2. 使用真实凭据发起一次请求，通过 `RecordingTransport` 捕获原始请求体、响应头、响应体（流式按 SSE 事件切分保存为 `.chunks.txt`）。
3. 剔除敏感头（`authorization`、`x-api-key`、`set-cookie`、`openai-organization` 等，白名单方式保留 `content-type`、限流头、请求 ID）。
4. 写入文件；元数据记录日期、供应商、用例与模型 ID。写入前检查每个文件不含 `sk-`、`Bearer ` 等模式与密钥原文，命中即失败。

fixture 一经录制不得手工修改；行为变化需重新录制并在 PR 中说明。

### 3.3 回放

`FixtureServer` 按文件重建响应：非流式经 `wiremock` 返回 JSON；流式由 `ferrin-testing` 内置的最小 hyper 1.x 服务器以 `text/event-stream` 逐帧发送，可配置分片间延迟以测试超时逻辑（见第 9 节 PV-026）。测试断言：

- 请求体快照（`insta::assert_json_snapshot!`）与录制的请求体一致。
- 规范化输出（`GenerateResult` 或事件序列）快照。
- 警告集合。

## 4. 核心层测试

- 生成循环：用 `MockLanguageModel` 编排多步响应，覆盖继续条件的每个分支（待审批、缺少执行函数、延迟结果、停止条件、`tool-calls` 之外的完成原因）。
- 流式管线：`simulate_stream` 构造事件序列；对每个阶段单独测试（工具执行注入顺序、部件 ID 重映射、重试边界丢弃、停止门）。
- 超时与重试：`tokio::time::pause()` + `advance()`，断言退避时长与 `Retry-After` 优先级。
- 取消：在不同阶段取消令牌，断言 `Error::Cancelled` 与任务清理（`JoinSet` 为空）。
- 审批：签名生成与校验、篡改输入检测、找不到调用、策略重解析。
- 结构化输出：五种策略的完整与部分解析；部分 JSON 修复的属性测试。

## 5. 属性测试

`proptest` 覆盖：

- `partial_json::repair`：对任意合法 JSON 的任意前缀，修复结果可解析。
- `SseDecoder`：对任意事件序列的任意字节切分方式，解码结果相同。
- `Usage::add`：结合律与 `None` 单位元。
- `ToolNameMapping`：重命名可逆。

## 6. 快照

- 使用 `insta`，快照文件与测试同目录 `snapshots/`。
- 审阅通过 `cargo insta review`；CI 中 `INSTA_UPDATE=no`，未接受的快照导致失败。
- 快照内容不含时间戳、随机 ID（测试注入 `SequentialIdGenerator` 与固定时钟）。

## 7. 覆盖率

- `coverage.yml` 以 `cargo llvm-cov nextest --workspace --all-features --lcov` 生成报告：`cargo llvm-cov report --summary-only` 的输出写入作业摘要，`lcov.info` 作为构建产物保留 14 天。
- 【决策】（2026-09-14）不接入外部覆盖率服务（如 Codecov）。依据：覆盖率数据留在 GitHub 内即可满足查看需求，避免向第三方上传源码级数据与维护额外令牌；原先的上传步骤因缺少令牌一直静默失败。需要 PR 级差异时可在本地用 `cargo llvm-cov --lcov` 与主分支的构建产物比较。
- 目标：`ferrin-spec`、`ferrin-schema`、`ferrin-core` 行覆盖 ≥ 85%；供应商 crate ≥ 75%。低于目标不阻断合并。

## 8. 测试数据与密钥

- 仓库中不出现真实密钥；fixture 录制脚本在写入前校验不含 `sk-`、`Bearer ` 等模式。
- 在线测试只在手动触发的 CI 工作流中运行，密钥来自 CI secret。

## 9. 待验证

- 【事实】（PV-026，`verification/pv026-sse-server`）`wiremock` 0.6.5 的 `ResponseTemplate` 只有完整内存体（`set_body_*`）与整响应延迟（`set_delay`），无分片或流式发送 API。原型用 `hyper` 1.11 + `http-body-util::StreamBody` 实现的服务器可按分片延迟发送，reqwest 客户端收到 3 个独立帧（到达时刻 0 ms、53 ms、105 ms，配置间隔 50 ms）。
- 【决策】`FixtureServer` 双后端：非流式 fixture 走 `wiremock`，流式 fixture 走 `ferrin-testing::sse_server`（hyper 1.x，约 80 行）；两者共用 fixture 文件格式与请求体快照断言。（2026-09-13 修订为单后端，见第 10 节与 [ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md)。）

## 10. 实现记录（2026-09-13）

- 【决策】测试代码与实现代码分离：每个 crate 的测试只位于 `tests/suite/*.rs`（由 `tests/all.rs` 以 `mod suite;` 汇总，子模块在 `tests/suite/mod.rs` 声明），`src/` 下不出现 `*_tests.rs` 文件或 `#[cfg(test)] mod tests`。需要验证的内部行为通过公共 API 触达；无法从公共 API 触达的辅助函数不单独测试。依据：实现文件只包含实现，读者与 `cargo xtask check-module-size` 的行数统计都不受测试代码干扰。
- 【决策】（[ADR 0013](../04-decisions/2026-09-13-0013-core-implementation-revisions.md) 第 4 项）`ferrin_testing::FixtureServer` 为单后端：内置 hyper 1.x 服务器按 `Fixture` 的 `FixtureBody::{Complete, Sse}` 回放，非流式 JSON 与流式 SSE 共用 `mount`/`mount_once`/`mount_times`/`mount_file`、`received()` 请求记录与 `set_chunk_delay` 分片延迟；`ferrin-testing` 不依赖 `wiremock`。供应商 crate 的测试可继续直接使用 `wiremock` 作为 dev 依赖。
- 【事实】`ferrin_testing::MockLanguageModel` 记录每次调用（`calls()`、`generate_calls()`、`stream_calls()`），构建器提供 `generate`/`generate_error`/`generate_repeat`/`generate_with` 与对应的 `stream_*` 脚本方法；`simulate_stream` 与 `SimulatedStream`（`initial_delay`、`chunk_delay`、`hang_at_end`）构造规范流；`RecordingTransport` 以头部白名单记录请求与响应，`redact_secrets` 在写入前替换 `sk-`、`Bearer ` 等模式。
- 【事实】核心层非文本模态的测试以内联实现规范 trait 的 mock（`EmbeddingModel`、`ImageModel`、`SpeechModel`、`TranscriptionModel`、`RerankingModel`、`VideoModel`、`Files`、`Skills`、`Batch`、`RealtimeModel`）驱动；实时会话测试用 `tokio-tungstenite` 在 `127.0.0.1` 起本地 WebSocket 服务器并回显子协议头。
- 【事实】`Fixture::load(dir, case)` 对同一 `case` 先找 `<case>.response.json`，再找 `<case>.chunks.txt`；两者同时存在时只回放前者。`ferrin-openai` 的流式用例因此以 `-stream` 后缀命名（`text-basic.response.json` 与 `text-basic-stream.chunks.txt`），第 3.1 节的目录示例按此理解。
- 【待验证】（PV-031）`ferrin-openai`、`ferrin-anthropic`、`ferrin-openai-compatible` 与 `ferrin-google` 的 fixture（`crates/providers/<crate>/tests/fixtures/`）在 `record-fixture` 命令实现（2026-09-14）之前依据供应商公开 API 文档的响应 schema 手工编写，不含真实请求 ID 与账户信息，尚未用该命令以真实凭据重新录制；第 3.2 节“fixture 一经录制不得手工修改”的规则自录制版本起适用。

## 11. 实现记录（2026-09-14，基准测试）

- 【决策】基准测试使用 `criterion` 0.8.2（工作区开发依赖，features `async_tokio`、`html_reports`），每个目标是一个 `benches/<name>.rs` 文件并在清单中声明 `[[bench]] harness = false`；文件在 crate 根放行 `clippy::unwrap_used`/`clippy::expect_used`（与 `tests/all.rs` 同理）。依据：criterion 提供统计置信区间、吞吐量单位与 HTML 报告，`async_tokio` 让异步管线直接在 tokio 运行时上计时，无需自写驱动。
- 【决策】基准测试不访问网络、不含 `sleep`：核心层用 `ferrin_testing::MockLanguageModel`（`generate_repeat`/`stream_repeat`，双步工具循环用 `generate_with` 加原子计数器交替返回工具调用与文本）；供应商适配器用 `FixtureServer` 回放本 crate `tests/fixtures/` 下的录制响应，每次迭代 `reset()` 后重新 `mount`，避免请求记录随迭代次数增长；门面 crate 的端到端基准以 `Fixture::sse_json` 现场合成 Responses API 文本流（不跨 crate 引用 fixture 文件，保证 `cargo package` 的自包含），并发用 `JoinSet`。依据：被测对象是 Ferrin 自身的开销（请求组装、HTTP、SSE 解码、事件映射、生成循环），真实供应商的延迟只会淹没这些差异。
- 【事实】目标清单（criterion 组 / 基准 id）：`ferrin-provider-util` `sse`（`sse_decoder/feed/{whole_body,4096,512,64}` 按分片大小喂入 2000 事件的 Responses 风格流，`decode_stream/4096`；吞吐量按字节）；`ferrin-schema` `partial_json`（`repair`、`parse_partial` 各取约 4 KiB 文档的 25/50/75/100 % 前缀，`serde_json_complete/100` 作对照）与 `schema`（`derived`、`openai_strict`、`validate/{typed_serde,raw_json_schema}`）；`ferrin-message` `prune`（`none`、`reasoning_all`、`tool_calls_all`、`tool_calls_before_last_4` × 40/200 条消息）；`ferrin-tool` `fingerprint`（`canonical_json/40_properties`、`fingerprint_tools/{5,20}`、`detect_tool_drift/20`）；`ferrin-core` `generate_text`（`single_step_prompt`、`history/{11,51}`、`tool_loop_two_steps`、`extract_reasoning_middleware`）与 `stream_text`（`text_stream/{100,1000}`、`events/1000`、`consume/1000`、`smooth_stream/word/1000`）；`ferrin-openai` `responses`、`ferrin-anthropic` `messages`、`ferrin-google` `generate_content`（各含 `generate/<case>` 与两个 `stream/<case>`，用例即 fixture 名）；`ferrin` `end_to_end`（`stream_text/{20,200}` 个增量、`concurrent_streams/{1,16,64}`，需 feature `openai`）。
- 【事实】（2026-09-14，开发机 macOS，Rust 1.98.1，参数 `--warm-up-time 0.5 --measurement-time 1 --sample-size 10`，仅验证可运行，非正式测量）11 个目标全部完成。量级：SSE 解码 2000 事件约 1.4 ms（分片 64 字节时约 1.5 ms）；部分 JSON 修复 4 KiB 前缀 2.7–11 µs；200 条消息裁剪 15–35 µs；`generate_text` 单步约 7 µs、双步工具循环约 39 µs；`stream_text` 1000 个增量约 0.7 ms（每增量约 0.7 µs）；三个适配器 `do_generate` 约 60 µs、`do_stream` 90–180 µs（含本地 HTTP 往返）；端到端 200 个增量约 1.6 ms，64 路并发（每路 50 个增量）约 8.3 ms。
- 【决策】基准结果不入库、不作为 CI 门禁：`ci.yml` 的 `clippy` 作业以 `--all-targets` 编译检查基准代码；`bench.yml` 只能手动触发（输入 `filter`），把 `target/criterion` 作为构建产物保留 30 天。依据：共享 runner 的计时噪声大，回归判定应在同一台机器上以 criterion 基线（`--save-baseline`/`--baseline`）比较。
- 【事实】`cargo bench --workspace -- <criterion 选项>` 会失败：没有 `[[bench]]` 的 lib 目标仍以 libtest harness 运行，libtest 拒绝 criterion 的选项（`error: Unrecognized option: 'sample-size'`，2026-09-14 以 `ferrin-spec` 验证）；位置参数形式的名称过滤两者都接受。因此 `just bench [filter]` 只传过滤器，criterion 选项须限定单个目标：`cargo bench -p <crate> --bench <name> -- --save-baseline <tag>`。

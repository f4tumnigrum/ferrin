# 待验证事项汇总

[English](../../05-appendix/02-pending-verification.md) | **简体中文**

本表汇总各文档中标注为【待验证】的事项。每项关闭时更新“状态”与“结论”，并同步修改来源文档。可复现的原型位于仓库根目录 `verification/`（独立 Cargo workspace，`just verify` 运行；基准以 `cargo run --release -p <crate>` 运行），结论中的数字均来自 2026-09-13 在 macOS arm64（Darwin 25.2.0，8 核，16 GiB）、Rust 1.98.1 上的运行结果。

状态：`closed` 已关闭（来源文档已改为【事实】/【决策】）；`open` 仍待验证（来源文档保留【待验证】并带编号）。

| 编号 | 事项 | 来源文档 | 结论 | 状态 |
| --- | --- | --- | --- | --- |
| PV-001 | `dynosaur` 0.3.1 能否生成满足 `Send + 'static` 的对象安全适配 | 总体架构 §5、ADR 0002 | 可以（`pv001-dynosaur`：`Arc<DynLanguageModel<'static>>` 跨 `JoinSet` 任务可用）；但生成类型为不定长结构体、需显式构造与 `?Sized`。决策：不采用，保留手写 `Dyn*` trait | closed |
| PV-002 | `data:` URL 解析采用 `data-url` 还是自实现；与朴素逗号切分解析的差异 | Prompt 转换 §7 | 朴素的“按逗号切分”解析不处理 `;base64` 标志与百分号编码（`pv002-data-url` 14 用例表）；决策：自实现 RFC 2397 解析，不引入 `data-url` | closed |
| PV-003 | 并发下载上限默认值 | Prompt 转换 §7、并发 §8 | 决策：默认 8（`DownloadOptions::max_parallel`），实现阶段以 `benches/download.rs` 跟踪 | closed |
| PV-004 | `schemars` draft-07 对 `Option`/`enum` 的形状；OpenAI 严格模式子集 | 工具系统 §10、结构化输出 §7、ADR 0004 | 形状已记录（`pv004-schema`）；严格模式由 `SchemaTransform::openai_strict()` 补齐 `additionalProperties: false`、全字段 `required` 与可空化，原型通过 | closed |
| PV-005 | `#[ferrin::tool]` 捕获非 `'static` 引用时的错误信息 | 工具系统 §10 | 基线诊断已录制（`pv005-static-capture/tests/ui/*.stderr`：E0597 与 “lifetime may not live long enough”，均定位到用户代码）；决策：宏对引用类型参数直接 `compile_error!` | closed |
| PV-006 | `JoinSet` + 有界通道在 >100 并行工具时的顺序与内存；容量 64 | 生成循环 §5、并发 §8 | 200/1000 任务下容量 1、64、1024 的耗时与峰值 RSS 无差异（2.9 MiB / 6.7 MiB）；顺序为完成顺序。容量 64 保持 | closed |
| PV-007 | `simulate_streaming` 下 `stream_text` 启动等待；是否需要 `start_eager()` | 生成循环 §5、ADR 0005 | 决策：不增加变体，等待时长为该中间件固有语义并写入文档 | closed |
| PV-008 | 部分输出深比较成本 vs 文本哈希 | 结构化输出 §7 | `Value` 深比较 96 KiB 对象 135 µs，序列化+哈希 180 µs，解析 936 µs（`pv008-partial-compare`）；保持深比较 | closed |
| PV-009 | `prepare_call` 的“移除外层设置”表达 | Agent §5 | 原型采用 `Override<T>::{Keep, Clear, Set}`（`pv009-override`）；2026-09-13 实现改为已填充默认值的普通字段（置 `None` 即移除），见 ADR 0013 第 2 项与 Agent §6 | closed |
| PV-010 | `extract_reasoning` 流式用例移植完整性 | 中间件 §4 | 用例集共 14 个（5 个非流式、9 个流式），已列为实现的最低测试集（中间件 §4） | closed |
| PV-011 | `embed_many` 的 `max_input_bytes_per_call` 度量口径 | 其他模态 §12 | 以 UTF-8 字节数度量；分块规则已记录并在 `embed::split_by_limits` 中按此实现（测试 `embed_many_splits_by_input_bytes`） | closed |
| PV-012 | 实时会话事件集合 | 其他模态 §12 | 服务端 22 种、客户端 8 种事件，已逐项列出（其他模态 §12） | closed |
| PV-013 | `Error` 枚举 ≤ 128 字节 | 错误模型 §5、ADR 0006 | 内联 360 字节；六个变体装箱后 56 字节，`const_assert!` 通过（`pv013-error-size`） | closed |
| PV-014 | `opentelemetry` 0.32 与 `tracing-opentelemetry` 0.33 配对 | 可观测性 §6、工具链 §6 | 编译并记录 span 成功（`pv014-otel`）；`opentelemetry-semantic-conventions` 的 `GEN_AI_*` 已弃用，改为自定义常量 | closed |
| PV-015 | reqwest 0.13.5 的 `resolve_to_addrs`/`redirect::Policy::none()`/每目标客户端成本 | HTTP §11、工具链 §6、ADR 0009 | API 保留；每客户端约 58 µs；固定地址请求不再解析 DNS（`pv015-reqwest`）；0.13 破坏性变更已记录 | closed |
| PV-016 | `http` 1.5 `HeaderMap` 非 ASCII 头值 | HTTP §11 | `from_str`/`from_bytes` 接受 UTF-8 字节，`to_str()` 拒绝；读取接口提供字节形式（`pv016-header-values`） | closed |
| PV-017 | MCP Streamable HTTP 头与恢复语义、协议版本常量 | MCP §2.2、§5、ADR 0010 | `2026-07-28` / `2025-11-25`；2026-07-28 移除会话、GET 流、恢复与服务端请求，新增 `Mcp-Method`/`Mcp-Name`/`Mcp-Param-*` 与 MRTR；决策为双代客户端 | closed |
| PV-018 | stdio 传输在 Windows 的管道与信号处理 | MCP §5 | 实现基线（`kill_on_drop`、单一写入任务整帧写出、`creation_flags`）在 `windows-2025` 作业通过：2026-09-14 run 34797869442 的三个 stdio 测试全部通过（fixture 服务器以 `python` 启动） | closed |
| PV-019 | 诱导请求 schema 与 MCP 规范的一致性 | MCP §5 | `{ message, requestedSchema }` / `{ action, content? }`，字段一一对应 | closed |
| PV-020 | `chunk` 超时 `Sleep::reset` 开销 | 并发 §8 | 每分片 73 ns（`Instant::now` 17 ns，`pv020-sleep-reset`）；保持方案 | closed |
| PV-021 | OpenAI `explicit_message_item_type`、`supports_web_search_sources_include` 是否保留 | Provider 实现指南 §8 | 对应 Azure Foundry 与 Bedrock Mantle 端点的已知差异；保留为 `OpenAiConfig` 字段 | closed |
| PV-022 | `hmac` 0.13 / `sha2` 0.11 / `subtle` 2.6 / `SecretBox<[u8]>` 配合 | 工具链 §6、安全 §8 | 编译通过（`pv022-crypto`）；`new_from_slice` 需 `hmac::KeyInit`，`verify_slice` 可用 | closed |
| PV-023 | `base64` 0.23、`rand` 0.10、`tokio-tungstenite` 0.30 的 API | 工具链 §6 | `Engine`/`prelude` 不变；`rand::rng()` + `RngExt`；tungstenite feature `rustls-tls-webpki-roots`（`pv023-api-changes`）。重复主版本以 warn 接受 | closed |
| PV-024 | `arc-swap` 是否需要 | 工具链 §2 | 不引入：默认注册表为 `OnceLock` 一次性设置，无热替换需求 | closed |
| PV-025 | `release-plz` 与 `git-cliff` 的选择 | 工具链 §4、版本与发布 §5 | `git-cliff`（`cliff.toml`）；`release-plz` 的 `version_group` 只统一“有变更的包”，与全 crate 同步递增规则冲突 | closed |
| PV-026 | `wiremock` 0.6.5 对 SSE 分片延迟的支持 | 测试规范 §9 | 无流式 API；hyper 1.x 最小服务器按 50 ms 间隔发出 3 帧（`pv026-sse-server`）；`FixtureServer` 原计划双后端，2026-09-13 实现为 hyper 单后端（ADR 0013 第 4 项） | closed |
| PV-027 | Dependabot 与 Renovate | CI §6 | Dependabot（`.github/dependabot.yml`，cargo 每周分组 + actions 每周） | closed |
| PV-028 | Windows 上 `lookup_host` 对 IPv4 映射 IPv6 地址的表示 | 安全 §8 | 判定前用 `Ipv6Addr::to_ipv4_mapped()` 规范化（`pv028-ipv4-mapped`）；macOS 与 Windows（run 34798946529，`windows-2025`）均返回 `[::1]` 与 `127.0.0.1`，无 IPv4 映射形式；规范化保留 | closed |
| PV-029 | 本机工具链更新到 1.98.1 | 工具链 §1 | `1.98.1-aarch64-apple-darwin` 已安装并被 `rust-toolchain.toml` 选中；全部门禁在其上运行 | closed |
| PV-030 | MCP 2026-07-28 的 `InputRequiredResult`/`inputRequests`/`inputResponses` 字段定义 | MCP §2.2.2、§5 | 2026-09-14 依据规范仓库 `schema/2026-07-28/schema.ts` 固定：`Result.resultType: "complete" \| "input_required"`；`InputRequiredResult { inputRequests?: { [key]: CreateMessageRequest \| ListRootsRequest \| ElicitRequest }, requestState?: string }`；客户端以 `params.inputResponses: { [key]: InputResponse }` 与 `params.requestState` 重试原请求。`ferrin-mcp` 已按此实现（仅处理 `elicitation/create` 输入请求） | closed |
| PV-031 | `ferrin-openai`、`ferrin-anthropic`、`ferrin-openai-compatible`、`ferrin-google` 手工编写的 fixture 与真实 API 响应的一致性 | 测试规范 §10、`docs/providers/openai.md`、`docs/providers/anthropic.md`、`docs/providers/openai-compatible.md`、`docs/providers/google.md` | fixture 依据供应商公开 API 文档的响应 schema 手工编写；`cargo xtask record-fixture` 已于 2026-09-14 实现（场景文件 `*.scenario.json`，见工作区布局 §6），需用真实凭据重新录制并比对快照 | open |
| PV-032 | `ferrin-policy` 的 `rego` feature 在 Windows MSVC 上的构建 | 策略化工具审批 §2.3、工具链 §5 | 2026-09-16，[CI run 35032650222](https://github.com/f4tumnigrum/ferrin/actions/runs/35032650222) 的 `test (windows-2025)` 作业在发布提交 `7f950ad` 上通过全 feature 构建、nextest 与 doctest。结论限于该托管运行器；自定义 MSVC 环境仍须安装 Spectre 缓解版 CRT 库。 | closed |

## 环境事实记录

- 设计时本机：macOS（Darwin 25.2.0，arm64，8 核，16 GiB），rustup 1.29.1；stable 默认工具链 1.98.0，另安装 `1.98.1-aarch64-apple-darwin`（含 clippy、rustfmt、rust-src、llvm-tools）。
- 命令行工具（2026-09-13 经 `cargo binstall` 安装）：cargo-nextest 0.9.144、cargo-deny 0.20.2、cargo-shear 1.13.4、cargo-insta 1.48.0、cargo-hack 0.6.45、cargo-semver-checks 0.50.0、cargo-llvm-cov 0.9.1、typos-cli 1.50.1、just 1.58.0、git-cliff 2.14.1、cargo-binstall 1.23.0；release-plz 0.3.165 已安装但未采用。
- 骨架质量门禁结果（2026-09-13）：`cargo check/clippy(-D warnings)/doc(-D warnings)` 通过，`cargo deny check` 四项通过，`cargo hack --each-feature` 50 组通过，`cargo nextest` 1 个测试通过，`typos` 通过；`cargo shear` 报告 292 项未使用依赖（骨架阶段预期，CI 暂不阻断）。
- crates.io 版本查询日期：2026-09-13；本机 cargo 使用 `rsproxy.cn` 稀疏索引镜像，`Cargo.lock`（395 个包）解析结果与查询一致。
- 外部规范：MCP 规范页面 `modelcontextprotocol.io/specification/2026-07-28/`（2026-09-13 访问）；OpenTelemetry GenAI 语义约定仓库 `open-telemetry/semantic-conventions-genai`（2026-09-14 访问）。

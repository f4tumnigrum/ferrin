# 逐模块对齐核查

[English](../../05-appendix/03-reference-parity.md) | **简体中文**

【决策】本核查遵循 [ADR 0026](../04-decisions/2026-09-17-0026-reference-sdk-parity.md)，仅覆盖 Ferrin 已有模块。参考版本为本地 Vercel AI SDK 的 `6c6c221` 提交，参考路径均相对该仓库。

【事实】起点为 Ferrin `49f0e1b`。其 903 项测试通过只构成现有回归基线，不证明参考行为已经对齐（来源：[能力补齐验证记录](../03-engineering/04-testing.md)）。

【决策】状态表示核查进度，不表示发布能力。未核查或进行中的项目不得描述为已验证。完成后在下方记录具体回归证据，明确保留尚未解决的差异。

| 模块 | 现有接口范围 | 参考路径 | 核查状态 |
| --- | --- | --- | --- |
| 供应商规范 | 供应商 trait、模型族、内容、用量和错误 | `packages/provider/src` | 部分完成：embedding 精度；其余类型约定仍待核查 |
| Schema 与 JSON | Schema 校验、方言转换、JSON 解析和修复 | `packages/provider-utils/src` | 已补解析/schema 回归；异步 schema 约定待补 |
| 应用消息 | 转换、文件来源和裁剪 | `packages/ai/src/prompt; packages/ai/src/generate-text/prune-messages.ts` | 已补转换和裁剪回归 |
| 供应商工具库 | 传输、SSE、配置、重试分类和 URL 处理 | `packages/provider-utils/src` | 已补流式上传和重试回归；保留安全边界 |
| 工具与审批 | 命名上下文、caller、动态工具、审批与重放 | `packages/provider-utils/src; packages/ai/src/generate-text` | 已补运行时约定回归 |
| Agent 与钩子 | 调用准备、参数校验、钩子和超时优先级 | `packages/ai/src/agent` | 已补准备、超时和回调回归 |
| 生成与步骤准备 | 状态、停止条件、模型和工具选择、步骤覆盖 | `packages/ai/src/generate-text` | 已补状态、sandbox 和结果回归 |
| 流与结构化输出 | 消费、转换、部分对象和数组元素 | `packages/ai/src/generate-text; packages/ai/src/text-stream` | 已补视图和输出回归；记录所有权差异 |
| 中间件与注册表 | 包装、内置中间件、供应商解析与默认配置 | `packages/ai/src/middleware; packages/ai/src/registry` | 已补回归修复；剩余差异见下文 |
| 嵌入与重排 | 分批、并发、顺序与结果映射 | `packages/ai/src/embed; packages/ai/src/rerank` | 已补 metadata 和生命周期回归；单值 embedding 差异待补 |
| 图像、语音与转录 | 请求准备、分批、生成数据与流 | `packages/ai/src/generate-image; generate-speech; transcribe; translate` | 已补请求、结果和音频回归 |
| 视频、文件、技能与批处理 | 轮询、资源、请求和结果映射、取消 | `packages/ai/src/generate-video; upload-file; upload-skill; batch` | 已补请求和资源回归；批处理引用约定待补 |
| 实时会话 | 会话生命周期、事件与工具执行 | `packages/ai/src/realtime` | 已补会话修复；事件和状态接口仍有差异 |
| MCP | 传输、协议生命周期、工具桥接和 OAuth | `packages/mcp/src` | 已补传输、工具、app 和 OAuth 回归；发现及鉴权仍有差异 |
| 遥测与 OpenTelemetry | 回调载荷、脱敏、span 和指标 | `packages/ai/src/telemetry; packages/otel/src` | 已补异步回调和模态 span；层级和载荷仍有差异 |
| 策略 | 决策归一化、默认值和 shadow 行为 | `packages/policy-opa/src` | 已补归一化、fallback 和观察器回归 |
| OpenAI、Anthropic 与兼容适配器 | 现有模型和资源族、供应商工具 | `packages/openai/src; anthropic/src; openai-compatible/src` | 已补工具 schema、请求、上传和用量回归 |
| Google、Azure 与 Voyage 适配器 | 现有模型和资源族、供应商工具 | `packages/google/src; azure/src; voyage/src` | 已补工具、资源、选项、鉴权和排名回归 |
| 门面、宏与测试 | 导出、feature 组合、工具派生和测试辅助 | `packages/ai/src/index.ts; packages/provider-utils/src; packages/test-server/src` | 门面、宏回归和 68 项 feature 构建通过；更新流式录制器 |

## 回归证据（2026-09-17）

【事实】回调、中间件、模态、MCP/OAuth 和 Google realtime 修改后的 `just test` 集成运行通过 1115 项，跳过 10 项需要凭据的 live 测试。工作区格式、Clippy、doctest、rustdoc、docs-lint、typos、module-size、依赖使用和 cargo-deny 检查通过。已重新生成全部十八个 API 快照，`just api-check` 通过。这不证明全部行为对齐，也不代表官方供应商验证。PV-031 保持开放。

【事实】`just features` 的 68 项独立 crate、feature 和示例构建全部通过。检查发现并修正了 core 遗漏 `futures-util/std` 声明，以及 Google 无条件编译的事件映射使用可选 schema 依赖的问题。没有修改工具链或外部依赖版本。

【事实】Agent/core 证据位于 `crates/ferrin-core/tests/suite/{agent_options,agent_overrides,prepare_step_parity,tool_contract_parity,hooks,result_aggregation,prompt_conversion,prompt_instructions,stream_views,stream_transform_parity,output_parity,telemetry_async}.rs`，覆盖可选参数校验、准备覆盖、并发 hooks、命名上下文、动态调用、sandbox 作用域、独立消费视图和 partial 输出边界。

【事实】基础模块证据包括 `ferrin-schema/tests/suite/{provider_schema,reference_partial,json}.rs`、`ferrin-message/tests/suite/prune.rs`、`ferrin-provider-util/tests/suite/upload_stream.rs` 和 `ferrin-testing/tests/suite/transport.rs`。Schema 语料包含 60 个不同的参考修复输入；供应商工厂对照 13 个 OpenAI、20 个 Anthropic 和七个 Google 工具 schema 检查。

【事实】供应商证据包括各适配器的 `tests/suite/reference_*` 用例与 fixture 快照，以及 Azure routing/security 和 Voyage reranking 测试。Voyage 直接适配器现转发空文档与零 `top_n`，保留解析后的排名顺序、重复项、数量与索引，并由 core 补充响应模型身份。修正后 14 项测试通过。代理录制的 fixture 响应仍与官方供应商响应区分。

## 已确认的剩余工作

【事实】Schema 构造和验证仍同步执行（`ferrin-schema/src/schema.rs`），参考支持异步 schema 生成与验证（`provider-utils/src/{schema,validate-types}.ts`）。当前解析修复没有关闭这一 API 差异。

【事实】`CallOptions.tools: Vec<_>` 无法区分省略工具和显式空列表，中间件默认值因此无法区分这两种输入。包装构造器也缺少显式身份选项，注册表供应商列表采用映射排序而非注册顺序。来源：`ferrin-spec/src/language_model/call_options.rs`、`ferrin-core/src/{middleware,registry}`；参考 `ai/src/{middleware,registry}`。

【事实】单值 embedding 复用批量限制并要求精确向量数量，而参考 `embed.ts` 直接调用供应商并选择第一个向量。这与已完成的逻辑生命周期工作分开记录（`ferrin-core/src/embed.rs`）。

【事实】Core 批处理接受裸 `BatchId`；参考会验证绑定供应商和版本的批处理引用。Realtime 暴露服务端流和本地工具，与参考会话 reducer、状态及回调 API 不同。来源：`ferrin-core/src/{batch,realtime}`，参考 `ai/src/batch/batch.ts` 和 `ai/src/realtime`。

【事实】MCP 在 state/issuer 验证、授权服务器凭据绑定和协议默认值修正后通过 111 项本地测试。OAuth 仍存在已验证发现重定向、协议头网络重试、完整可选 metadata 验证和直接 exchange/refresh 辅助函数自定义鉴权差异。文件字符串简写仍表示文本，而参考表示 base64；可使用显式字节/文本输入。参见 [MCP](../01-architecture/15-mcp.md) 与 `ferrin-core/src/files.rs`。

【事实】OpenTelemetry 仍缺参考操作/步骤 span 层级、补充属性组、enrichment 回调和 GenAI 消息格式。Telemetry 仍采用逐调用注册、显式开启内容记录和整块上下文开关；参考全局注册、记录默认值、逐属性上下文过滤和完整事件载荷仍待对齐。详见[可观测性](../01-architecture/13-observability.md)。

## 表示与安全边界

【决策】明确记录 Rust 轮询/所有权、仅 JSON 运行和工具上下文、`usize` 索引、Unicode 标量和有限 JSON 数值表示。这些差异不能证明任意更严格的适配器验证等价。共享 completion 和事件视图在不启动游离任务的前提下提供 Rust 消费约定。

【决策】按照 ADR 0026，保留 HTTPS、私网限制、DNS 固定、JSON/HTTP 数据上限、秘密脱敏和签名审批。Reasoning 流仍比参考更严格地补齐未完成生命周期事件。上述剩余源码差异意味着不能宣称所有已有模块已严格对齐。

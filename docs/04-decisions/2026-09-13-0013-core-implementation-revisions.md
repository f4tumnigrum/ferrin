# 0013: 核心层实现阶段对既有决策的修订

- 状态：accepted
- 日期：2026-09-13
- 相关：[Agent](../01-architecture/09-agent.md)第 2、5 节；[其他模态](../01-architecture/11-other-modalities.md)第 9 节；[可观测性](../01-architecture/13-observability.md)第 3 节；[测试规范](../03-engineering/04-testing.md)第 9 节；PV-009、PV-026

## 背景

`ferrin-core` 与 `ferrin-testing` 的实现过程中，五处已记录的【决策】在编译器约束或 API 一致性面前需要修订。每处修订都改变了设计文档中的既有决策文本，按[架构决策记录流程](../03-engineering/07-adr-process.md)第 6 节集中记录于本 ADR；对应设计文档段落已同步加注日期。

## 决策

1. **Agent 调用选项不再要求 `DeserializeOwned + JsonSchema`。** `Agent::Options` 的约束为 `Send + 'static`；`ToolLoopAgentBuilder::call_options::<O>()` 只切换泛型参数。
2. **`PreparedCall` 使用已填充默认值的普通字段，不使用 `Override<T>`。** `prepare_call` 接收 `PrepareCallInput { options, defaults: PreparedCall }`，其中 `defaults` 是 Agent 设置与本次调用参数合并后的有效值；函数返回修改后的 `PreparedCall`，把某字段设为 `None` 即“移除外层设置”。
3. **实时会话的事件流项为 `Result<RealtimeServerEvent, Error>`，本地工具在连接前通过构建器登记。** `RealtimeSession` 实现 `Stream<Item = Result<RealtimeServerEvent, Error>>`；`realtime_session(model).tools(ToolSet)` 在 `connect()` 时把工具定义并入 `session-update`。
4. **`FixtureServer` 单后端。** `ferrin-testing` 内置的 hyper 1.x 服务器同时回放非流式 JSON 与流式 SSE fixture；`ferrin-testing` 不依赖 `wiremock`。
5. **其他模态共用一个 `ferrin.modality` span。** 操作名以 `gen_ai.operation.name` 字段区分（`embed`、`rerank`、`image`、`speech`、`transcription`、`video`、`upload_file`、`start_batch` 等），不再按模态命名 span。

## 依据

1. 原设计以 schema 在运行期校验外部传入的 JSON 选项；Rust 中 `options` 是调用方以静态类型构造的值，不存在需要校验的反序列化边界。保留 `DeserializeOwned + JsonSchema` 会迫使每个选项类型派生 `JsonSchema`，却没有任何代码消费该 schema。
2. `Override<T>` 的三态用于表达“保持外层值 / 清除 / 设为新值”。当 `prepare_call` 直接收到已合并的有效值时，“保持”就是不修改字段，“清除”就是置 `None`，第三态失去意义；`PreparedCall` 与 `CallSettings` 复用同一组字段类型，避免两套平行结构。PV-009 原型比较的是 `Override<T>` 与 `Option<Option<T>>`，未考虑“先合并再交给回调”的形态。
3. 原设计以错误回调报告传输错误与工具执行失败；纯事件流没有对应通道，`Item = RealtimeServerEvent` 会让连接失败静默变成流结束。`Result` 项让消费者区分供应商发出的 `Error` 事件与本地失败。工具定义必须包含在首个 `session-update` 中，因此工具集是连接前的配置而非连接后的方法。
4. 两类 fixture 共用请求记录、请求体断言与分片延迟逻辑；一个后端只需一组挂载 API 与一个监听端口。`wiremock` 无流式发送能力（PV-026），保留它只为非流式路径增加一份依赖与第二套匹配器。
5. `tracing` 的 span 名必须是静态字符串字面量（`tracing::info_span!` 的第一个参数），按模态拼接名称需要为每个模态写一个宏调用；统一 span 名加操作字段与 OpenTelemetry GenAI 语义约定中 `gen_ai.operation.name` 的用法一致，订阅方按字段过滤。

## 备选方案

- 保留 `Override<T>`：需要在 `PreparedCall` 与 `CallSettings` 之间做双向转换，且 `Override::Keep` 在有效值已知时无对应语义。
- 实时事件流项为 `RealtimeServerEvent`，错误走独立回调：与流式文本 API 的 `StreamEvent::Error` 风格不一致，且回调在 Rust 中需要额外的 `Arc<dyn Fn>` 参数。
- `FixtureServer` 双后端：见依据第 4 条。
- 每模态一个 span 名：实现成本高于收益，且与文本路径的 `ferrin.generate_text` / `ferrin.stream_text` 同级名称过多。

## 影响

- [Agent](../01-architecture/09-agent.md)第 1、2、5 节、[其他模态](../01-architecture/11-other-modalities.md)第 9 节、[可观测性](../01-architecture/13-observability.md)第 3 节表格、[测试规范](../03-engineering/04-testing.md)第 9 节已加注修订说明。
- PV-009 的结论改为“已被本 ADR 第 2 项取代”；PV-026 的结论改为“单后端”。
- `verification/pv009-override` 原型保留为历史记录，不再对应实现。

# 0005: 流式结果交付形态

- 状态：accepted
- 日期：2026-09-13
- 相关：[生成循环与流式管线](../01-architecture/07-generation-loop-and-streaming.md)第 3 节

## 背景

【事实】JavaScript 生态中流式 SDK 的常见形态是同步返回结果对象、通过 tee 提供多个消费视图（完整事件流、文本流、部分输出流）、在任一属性被访问时自动消费流，并让供应商错误进入流而不抛出；该形态依赖无界缓冲与隐式驱动。

Rust 的 `Stream` 是拉取式、单消费者；多视图需要无界缓冲或后台任务。

## 决策

1. `stream_text(...).await` 在首步骤模型请求建立后返回 `Result<StreamTextResult, Error>`。
2. `StreamTextResult` 提供单一事件流与 `Completion` 句柄，可 `split()`；派生视图（`text_stream`、`partial_output_stream`）消费同一底层流。
3. 不提供内置多路复用；不做无界缓冲；未消费的流不推进。
4. 后续步骤错误以 `StreamEvent::Error` 传递并由 `Completion` 返回 `Err`。

## 依据

- 背压与所有权语义清晰；无隐藏后台任务与内存增长。
- 配置与鉴权错误在调用点以 `?` 处理，符合 Rust 生态惯例（`reqwest::send().await?`）。
- 应用需要多路消费时可使用 `tokio::sync::broadcast`，成本由应用显式承担。

## 备选方案

- 完全模拟 tee：需要 `Arc<Mutex<VecDeque>>` 与多消费者游标，复杂且易泄漏。
- 后台任务驱动 + `watch` 通道：隐式 `spawn`，与“库不隐式创建任务”原则冲突。

## 影响

- API 文档必须强调“必须消费事件流或调用 `consume()`”。
- 【决策】（PV-007）`simulate_streaming` 下首步骤等待时长等于一次非流式调用，属于该中间件的固有语义，在其文档中说明；不增加 `start_eager()`（见[生成循环与流式](../01-architecture/07-generation-loop-and-streaming.md)第 5 节）。

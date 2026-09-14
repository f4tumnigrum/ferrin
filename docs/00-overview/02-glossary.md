# 术语表

| 术语 | 定义 | Ferrin 对应标识符 |
| --- | --- | --- |
| Provider（供应商） | 提供模型能力的服务方及其适配器实现，负责将规范层调用转换为具体 API 请求。 | `ferrin_spec::Provider` |
| Provider 规范 | Provider 适配器必须实现的一组 trait 与数据类型。规范版本变化即适配器契约变化。 | `ferrin_spec::SPEC_VERSION` |
| 语言模型 | 接收 Prompt 并生成文本、推理、工具调用等内容的模型。 | `ferrin_spec::LanguageModel` |
| Prompt（规范层） | 发送给语言模型的标准化消息序列，仅包含规范定义的内容部件。 | `ferrin_spec::Prompt` |
| 应用侧消息 | 应用构造的消息，允许便捷形式（字符串、字节、URL），经标准化转换为规范层 Prompt。 | `ferrin_message::Message` |
| 内容部件 | 消息中的最小内容单元：文本、文件、推理、工具调用、工具结果、审批请求、自定义内容、来源。 | `*Part` 类型 |
| 步骤（Step） | 生成循环中的一次模型调用及其后续工具执行。 | `ferrin_core::StepResult` |
| 停止条件 | 决定生成循环是否在存在工具结果时继续的谓词。 | `ferrin_core::StopCondition` |
| 工具 | 模型可以调用的函数或供应商内置能力，含输入 Schema、可选执行函数与元数据。 | `ferrin_tool::Tool` |
| 工具集 | 工具名到工具的有序映射。 | `ferrin_tool::ToolSet` |
| 供应商执行工具 | 由供应商在其服务端执行的工具（如网页搜索），其结果随模型响应返回。 | `ToolKind::ProviderExecuted` |
| 供应商定义工具 | 由供应商定义 Schema 但在客户端执行的工具（如计算机操作）。 | `ToolKind::ProviderDefined` |
| 动态工具 | 运行期才能确定 Schema 的工具（如 MCP 工具），输入输出类型为 JSON 值。 | `ToolKind::Dynamic` |
| 工具审批 | 工具执行前需要应用或用户确认的机制，产生审批请求与审批响应部件。 | `NeedsApproval`、`ToolApprovalRequestPart` |
| 延迟结果 | 供应商执行工具在当前响应中未返回结果，需在后续步骤中补齐。 | `supports_deferred_results` |
| 结构化输出 | 要求模型输出符合 Schema 的 JSON，并解析为类型化值。 | `ferrin_core::Output` |
| 部分 JSON 修复 | 对流式传输中不完整的 JSON 文本进行补全以得到可解析的中间值。 | `ferrin_schema::partial_json` |
| 中间件 | 包装语言模型的组件，可改写参数、包装生成与流式调用。 | `ferrin_core::LanguageModelMiddleware` |
| 注册表 | 以 `provider_id:model_id` 字符串解析模型实例的组件。 | `ferrin_core::ProviderRegistry` |
| 遥测集成 | 接收生成生命周期回调的可插拔组件。 | `ferrin_core::Telemetry` |
| 警告 | 供应商或核心在不中断调用的前提下报告的能力缺失、兼容降级或弃用信息。 | `ferrin_spec::Warning` |
| 用量 | 一次或多次模型调用的 token 统计。 | `ferrin_spec::Usage` |
| 完成原因 | 模型停止生成的原因，含统一枚举与供应商原始值。 | `ferrin_spec::FinishReason` |
| 供应商选项 | 按供应商键分组的透传 JSON 对象，用于供应商特有请求参数。 | `ProviderOptions` |
| 供应商元数据 | 按供应商键分组的透传 JSON 对象，用于供应商特有响应信息。 | `ProviderMetadata` |
| 供应商引用 | 供应商侧资源标识映射（如上传后的文件 ID）。 | `ProviderReference` |
| MCP | Model Context Protocol，通过 JSON-RPC 暴露工具、资源与提示的协议。 | `ferrin_mcp` |
| 取消令牌 | 用于协作式取消异步操作的句柄。 | `tokio_util::sync::CancellationToken` |
| Fixture | 录制的供应商原始响应（含 SSE 分片），用于离线回放测试。 | `tests/fixtures/` |

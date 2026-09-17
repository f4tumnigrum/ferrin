# 0021：持久化步骤状态与独立运行上下文

[English](../../04-decisions/2026-09-17-0021-agent-runtime-context.md) | **中文**

- 状态：proposed
- 日期：2026-09-17
- 相关：[生成循环](../01-architecture/07-generation-loop-and-streaming.md)、[Agent](../01-architecture/09-agent.md)、[工具系统](../01-architecture/06-tool-system.md)

## 背景

【事实】 Ferrin 的共享步骤准备逻辑每次都从初始调用重建消息、指令和工具上下文（`crates/ferrin-core/src/generate_text/inputs.rs`，2026-09-17 检查）。本地 Vercel AI SDK 参考实现会延续步骤覆盖，并提供独立的 `runtimeContext` 与 `toolsContext`（`packages/ai/src/generate-text/prepare-step.ts`）。

## 决策

【决策】 每次生成调用分别维护消息、指令、工具上下文和运行上下文的演进状态。`prepare_step` 覆盖替换当前及后续步骤的相应状态；消息覆盖后只追加新生成的响应。回调仍可读取初始消息和完整响应历史。模型、工具选择及顺序、采样设置的覆盖仍只作用于当前步骤。

【决策】 调用和 Agent 构建器、Agent 调用准备、步骤准备、审批上下文、生命周期钩子及步骤结果均提供 `runtime_context: Option<JsonValue>`。它属于应用状态，不进入供应商参数或工具执行上下文。覆盖的 `None` 表示保持原值，JSON `null` 表示显式替换。每步记录两类上下文，旧结果缺失字段时仍能反序列化。审批重放使用新调用的初始上下文。

【决策】 遥测集成副本仅在显式启用 `include_runtime_context` 时保留运行上下文，仅在启用 `include_tools_context` 时保留工具上下文，两者默认均关闭。应用钩子与返回结果保留上下文。

【决策】 工具定义元数据传播至解析调用和工具结果，包括无效调用、供应商结果及流式初步结果。供应商元数据保持独立。持久化结果缺失新字段时解析为 `None`。

## 理由与替代方案

【决策】 JSON 运行状态遵循 Ferrin 的可序列化上下文约定，避免给整个生成 API 增加泛型。每次调用独立状态避免可复用 Agent 跨调用泄漏。保留完整响应历史使提示压缩后仍可审计输出。

【决策】 每步重置覆盖会迫使回调自行维护状态，并可能恢复为压缩而删除的消息。向执行器传递运行状态会混淆应用生命周期数据和通过 schema 验证的工具上下文。

## 影响

【决策】 本变更修改后续步骤语义，并为公共结构体及流事件增加字段，下游 Rust 结构体字面量需要更新。实现属于尚未发布的提案，等待维护者评审；本文不声明已被接受。不需要改变供应商协议或外部依赖。

# 0014: `ferrin-openai-compatible` 的模型族与 Responses 模式

[English](../../04-decisions/2026-09-13-0014-openai-compatible-model-families.md) | **简体中文**

- 状态：accepted
- 日期：2026-09-13
- 相关：[Provider 适配器实现指南](../01-architecture/17-provider-implementation-guide.md)第 6、8、11 节；[crate 划分](../01-architecture/02-crates.md)第 1 节；[OpenAI 兼容端点](../providers/openai-compatible.md)

## 背景

实现指南第 8 节的【决策】在保留 `OpenAiConfig` 的 `explicit_message_item_type` 与 `supports_web_search_sources_include` 两个字段之外，附带了一条“`ferrin-openai-compatible` 在 Responses 模式下透传”的条款。实现 `ferrin-openai-compatible` 时确认：本 crate 的定位只覆盖 Chat Completions、Completions、嵌入与图像四类模型，这两个标志只对直接复用 Responses 模型实现的专用适配器（Azure、Bedrock Mantle 一类端点）有意义；crate 划分文档对本 crate 的描述也只列出这四类。该条款没有可实现的对象，按[架构决策记录流程](../03-engineering/07-adr-process.md)第 6 节记录修订。

## 决策

1. `ferrin-openai-compatible` 提供且仅提供四个模型族：`<name>.chat`、`<name>.completion`、`<name>.embedding`、`<name>.image`；不提供 Responses 模式。
2. 需要以 Responses API 访问兼容端点时，使用 `ferrin-openai` 的 `OpenAiSettings { base_url, name, .. }`；`OpenAiConfig` 的两个字段保留不变（第 8 节决策的前半部分不受影响）。
3. 实现指南第 8 节中的透传条款撤销，段落加注日期与本 ADR 链接。

## 依据

- Responses 的请求与响应形状是 OpenAI 专有的，`ferrin-openai` 已完整实现并允许配置 `base_url` 与 `name`；在兼容 crate 中再实现一份会产生两套相同的转换代码。
- crate 划分文档规定 `ferrin-openai-compatible` 只依赖 `ferrin-spec`、`ferrin-schema` 与 `ferrin-provider-util`；复用 `ferrin-openai` 的 Responses 模型需要新增 crate 间依赖，改变依赖方向。
- 兼容端点普遍宣称的是 Chat Completions 兼容，Responses 兼容的端点极少，且这类端点通常需要专用适配器。

## 备选方案

- 在 `ferrin-openai-compatible` 中依赖 `ferrin-openai` 并 re-export 其 Responses 模型：改变 crate 依赖方向，且把 `ferrin-openai` 的全部依赖带入兼容 crate；拒绝。
- 在 `ferrin-openai-compatible` 中独立实现 Responses 模型：重复代码，维护两份事件映射；拒绝。

## 影响

- 实现指南第 6 节按实际导出更正类型名，第 8 节加注修订说明，第 11 节记录实现。
- `ferrin` 门面的 `openai-compatible` 特性与 `ferrin::providers::openai_compatible` 的 re-export 计划不变。

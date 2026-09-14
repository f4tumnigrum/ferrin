# 0003: JSON 值类型与序列化格式

[English](../../04-decisions/2026-09-13-0003-json-value-and-serialization.md) | **简体中文**

- 状态：accepted
- 日期：2026-09-13
- 相关：[核心数据模型](../01-architecture/03-core-data-model.md)

## 背景

【事实】供应商选项与元数据在各供应商 API 中都是任意 JSON 对象，需要按供应商键分组透传；面向前端转发的事件流需要一种跨语言易于消费的联合类型编码，带 `type` 标签的对象是 JSON 生态的通行做法。

【事实】OpenAI 与 Anthropic 的提示缓存以请求前缀（含工具定义）为键，工具定义顺序变化会导致缓存未命中，因此工具与对象键的序列化顺序必须稳定。

## 决策

1. 使用 `serde_json::Value`/`Map` 作为 JSON 值类型，不定义自有类型。
2. `ferrin-spec` 启用 `serde_json` 的 `preserve_order`。
3. 所有公共枚举以 `#[serde(tag = "type")]`（消息为 `role`）序列化，标签 kebab-case，字段 snake_case，二进制为 base64，时间为 RFC 3339。
4. 公共枚举与可扩展结构体标记 `#[non_exhaustive]`。

## 依据

- `serde_json` 是事实标准，自定义类型只增加转换成本。
- 保序保证工具定义与 fixture 输出的确定性。
- 内部标签格式是 JSON 生态最常见的联合类型编码，便于跨语言前端消费 Ferrin 的事件流。

## 备选方案

- 自定义 `JsonValue` 枚举：与生态隔离。
- 外部标签（serde 默认）序列化枚举：与主流 SSE/JSON 事件格式不一致。

## 影响

- `preserve_order` 经 feature 统一对下游生效，需在门面 crate 文档说明。
- 下游对 `#[non_exhaustive]` 枚举的 `match` 需通配分支。

# 0019：可失败的供应商 Schema 转换

[English](../../04-decisions/2026-09-15-0019-fallible-schema-transforms.md) | **简体中文**

- 状态：accepted
- 日期：2026-09-15
- 关联：[ADR 0004](2026-09-13-0004-schema-library-and-dialect.md)、[工具系统](../01-architecture/06-tool-system.md)

## 背景

【事实】OpenAI strict 对象要求 `additionalProperties: false`（[结构化输出指南](https://platform.openai.com/docs/guides/structured-outputs)）。以 Schema 值的 `additionalProperties` 表示任意键字典时，无法在此限制下保留其值。

【事实】原来的不可失败 `SchemaTransform` API 原样保留字典 Schema，因此 OpenAI strict 转换结果仍可能违反该要求（审查 F04；回归案例位于 `crates/ferrin-schema/tests/suite/transform.rs`）。

## 决策

【决策】`SchemaTransform::apply`、`SchemaTransform::applied` 和 `to_openai_strict` 返回 `Result<_, SchemaError>`。OpenAI strict 在修改前检查 Schema 全部层级，包括 draft-07 `dependencies` 中的 Schema 值，拒绝 Schema 值或显式为 true 的 `additionalProperties` 以及 `patternProperties`，返回 `SchemaError::UnsupportedTransform`。应用可以关闭 strict 或自行提供受支持的表示。

【决策】`Schema::transformed` 同样返回 `Result` 并立即执行转换，保留原有校验器。默认派生 Schema 继续使用不可失败的附加属性转换，保留字典 Schema。

## 依据与备选方案

【决策】不能静默丢弃字典值，也不能把对象改为键值项列表，否则会改变应用的输入输出表示。保留不可失败 API 只能 panic 或返回已知不受支持的结果；显式错误允许供应商在发请求前失败。

## 影响

【决策】这是 Rust API 的破坏性变更：调用方必须传播或处理新增的 `Result`。关闭附加属性和删除属性名称的原有不可失败自由函数保持可用。本决策细化 ADR 0004，不改变默认生成方言和类型校验器。

# 0004: Schema 库与方言

- 状态：accepted
- 日期：2026-09-13
- 相关：[结构化输出](../01-architecture/08-structured-output.md)、[工具系统](../01-architecture/06-tool-system.md)

## 背景

【事实】OpenAI、Anthropic 与 Google 的函数工具参数与结构化输出都以 JSON Schema 描述，但各自只支持一个子集：OpenAI 严格模式要求 `additionalProperties: false` 且全部属性列入 `required`，Google 接受 OpenAPI 3.0 风格的 schema 子集；适配器因此都需要对生成的 schema 做供应商特定变换。

## 决策

1. `ferrin-schema` 以 `schemars` 1.2.2 派生 JSON Schema，默认 draft-07 设置；以 `serde` 反序列化作为类型化校验。
2. 无 Rust 类型的动态 Schema（MCP 工具）通过可选 feature `json-schema-validation` 使用 `jsonschema` 0.56.0 校验。
3. `Schema<T>` 结构：惰性 JSON Schema + 校验闭包，可由派生、原始 JSON Schema 或自定义校验器构造。

## 依据

- `schemars` 是 Rust 生态中与 `serde` 配合最广泛的 Schema 派生库；类型即 Schema，减少重复定义。
- draft-07 使用 `definitions` 与 `type` 数组等各供应商解析器普遍支持的结构，适配器变换只需处理少量关键字。
- 动态校验放入可选 feature，使不使用 MCP 的应用不引入 `jsonschema` 及其依赖。

## 备选方案

- 只支持原始 JSON Schema：应用需手写 Schema，易与类型脱节。
- 默认 draft 2020-12：更现代，但需要重新验证所有适配器变换。

## 影响

- 【事实】（PV-004）`schemars` 1.2.2 draft-07 对 `Option` 生成 `type: [T, "null"]` 或 `anyOf: [$ref, {type: null}]`、对枚举生成 `enum`/`oneOf`；OpenAI 严格模式所需的 `additionalProperties: false`、全字段 `required` 与可空化由 `SchemaTransform::openai_strict()` 变换补齐，原型 `verification/pv004-schema` 已验证（见[工具系统](../01-architecture/06-tool-system.md)第 10 节）。
- 公共 API 中出现 `schemars::JsonSchema` trait bound，纳入第三方类型白名单。

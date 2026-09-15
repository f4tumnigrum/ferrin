# 结构化输出

[English](../../01-architecture/08-structured-output.md) | **简体中文**

结构化输出位于 `ferrin-core::output`，Schema 与部分 JSON 修复位于 `ferrin-schema`。

## 1. 输出策略

【决策】`Output` 定义五种策略，每种提供响应格式、完整解析与部分解析：

| 策略 | 响应格式 | 完整解析 | 部分解析 |
| --- | --- | --- | --- |
| 文本 | `text` | 原文 | 原文 |
| 对象（schema、可选名称与描述） | `json` + schema | 解析 JSON 并按 schema 校验，失败报 `NoObjectGenerated` 错误 | 修复部分 JSON 后返回深部分值 |
| 数组（元素 schema） | `json` + `{elements: [element]}` 包装 schema | 解析后取 `elements` | 逐元素输出已完整的元素 |
| 选择（候选列表） | `json` + `{result: {enum: options}}` 包装 | 取 `result` | 匹配前缀唯一时给出候选 |
| 任意 JSON（可选 schema） | `json` | 任意 JSON 值 | 修复后的部分值 |

【决策】Ferrin 只提供 `generate_text(...).output(Output::object::<T>())` 一条路径，不提供独立的 `generate_object` 函数。依据：独立入口与 `output()` 的能力完全重叠，单一路径减少 API 表面与重复的循环实现。

```rust
pub struct Output<T> { strategy: OutputStrategy, _marker: PhantomData<T> }

impl Output<String> { pub fn text() -> Self; }
impl<T: DeserializeOwned + JsonSchema> Output<T> { pub fn object() -> Self; pub fn object_with(schema: Schema<T>) -> Self; }
impl<T: DeserializeOwned + JsonSchema> Output<Vec<T>> { pub fn array() -> Self; }
impl Output<String> { pub fn choice(options: impl IntoIterator<Item = impl Into<String>>) -> Self; }
impl Output<JsonValue> { pub fn json() -> Self; pub fn json_with_schema(schema: JsonValue) -> Self; }
```

`GenerateText<O>` 构建器的 `.output(Output<T>)` 把结果类型改为 `GenerateTextResult<T>`。

【事实】 数组输出将元素 Schema 嵌入 `properties.elements.items` 时保留本地引用语义：重定位根和指针引用，保留命名锚点及独立 `$id` 作用域，不改写 `const` 等字面量（2026-09-15，`tests/suite/output.rs`）。

## 2. 解析条件

【决策】结构化输出只在最后一步满足以下条件时解析：完成原因为 `stop`，或完成原因不是 `tool-calls` 且文本非空。不满足时返回 `NoOutputGenerated` 错误；解析失败返回 `NoObjectGenerated` 错误（携带文本、响应、用量、完成原因与原因）。依据：以工具调用结束的步骤不含最终答案，对其解析只会产生误报。

【决策】Ferrin 中不满足条件时 `generate_text` 返回 `Error::NoOutputGenerated { steps }`，而不是把 `output` 置为空。依据：`output` 的类型是 `T` 而非 `Option<T>`；返回错误使调用方必须处理“模型未产出结构化结果”的情形。

## 3. 部分 JSON 修复

【决策】部分 JSON 修复是一个状态机：扫描输入维护栈（对象、数组、字符串、字面量、数字），在输入截断处按栈状态补齐引号、括号，并删除不完整的字面量（如 `tru`）与尾随逗号；部分解析先尝试直接解析，失败后修复再解析，并返回解析状态（成功、修复后成功、失败）。依据：流式结构化输出需要在每个分片后给出可用的部分值，栈式修复比正则替换更能覆盖嵌套结构。

【决策】`ferrin_schema::partial_json::repair(&str) -> Cow<str>` 与 `parse_partial(&str) -> PartialParse { value: Option<JsonValue>, state }` 移植该状态机；以 `proptest` 验证“对任意合法 JSON 的任意前缀，修复结果可解析且是原值的前缀近似”。

【事实】2026-09-13 实现：`PartialParseState` 只有 `SuccessfulParse`、`RepairedParse`、`FailedParse`（空字符串归入 `FailedParse`）。数组首元素为孤立的 `-` 时（输入 `[-`）输出 `[]` 而非不可解析的 `[-]`。`proptest` 用例对随机 JSON 值（紧凑与美化两种格式）的每个字符边界前缀断言修复结果可解析。

## 4. 部分输出流

【决策】部分输出流在每次文本增量后重新解析累计文本，仅当解析出的值与上一次不同时发出；数组策略另发出新完成的元素。依据：去重避免界面在无变化的分片上重复渲染。

```rust
pub struct PartialOutput<T> {
    pub value: JsonValue,           // repaired partial JSON
    pub typed: Option<T>,           // Some when the partial value already deserializes into T
}
```

【决策】部分输出以 JSON 值为主、类型化值为辅。依据：Rust 没有把类型的全部字段递归改为可选的类型级操作，类型化的部分值需要为每个类型另行定义；应用通常只需部分 JSON 做 UI 预览，而在结构完整时需要类型化值。数组策略提供 `element_stream() -> impl Stream<Item = T>`。

## 5. Schema

### 5.1 抽象

【决策】`Schema<T>` 由（可惰性计算的）JSON Schema 与校验函数组成，可由 Rust 类型（`schemars`）、手写 JSON Schema 或自定义校验函数构造。依据：工具与结构化输出既需要发送给供应商的 JSON Schema，也需要在本地校验模型输出，二者绑定在一个值上可以避免不一致。

```rust
pub struct Schema<T> {
    json_schema: LazySchema,                       // OnceLock<JsonValue> or precomputed
    validate: Arc<dyn Fn(JsonValue) -> Result<T, TypeValidationError> + Send + Sync>,
}

impl<T: DeserializeOwned + JsonSchema> Schema<T> {
    pub fn derived() -> Self;                       // schemars + serde
}
impl Schema<JsonValue> {
    pub fn from_json_schema(schema: JsonValue) -> Self;   // dynamic; validates via `jsonschema` when the feature is on
}
impl<T> Schema<T> {
    pub fn with_validator(self, f: impl Fn(JsonValue) -> Result<T, TypeValidationError> + Send + Sync + 'static) -> Self;
    pub fn json_schema(&self) -> &JsonValue;
    pub fn validate(&self, value: JsonValue) -> Result<T, TypeValidationError>;
}
```

【事实】2026-09-13 实现在此基础上补充：`Schema::<T>::typed_from_json_schema(JsonValue)`（原始 Schema + 反序列化为 `T`，开启 `json-schema-validation` 时先做 JSON Schema 校验）、`Schema::lazy(FnOnce() -> JsonValue, validator)`、`with_json_schema_and_validator`、`Schema::<JsonValue>::empty_object()`/`any()`、`transformed(SchemaTransform)`（最初惰性重写；自 2026-09-15 返回 `Result` 并立即重写，校验不变，见 [ADR 0019](../04-decisions/2026-09-15-0019-fallible-schema-transforms.md)）与 `erased() -> Schema<JsonValue>`（运行原校验、返回原 JSON 值）。`json_schema` 以 `LazyLock<JsonValue, Box<dyn FnOnce>>` 承载并由 `Arc` 共享，`Clone` 共享缓存与校验器。`from_json_schema` 在 `json-schema-validation` 关闭时不做校验（所有值通过）。
### 5.2 Schema 方言

【事实】各供应商只接受 JSON Schema 的子集，适配器需要做供应商特定变换（Anthropic 清理不支持的关键字，OpenAI 严格模式要求 `additionalProperties: false` 与全字段 `required`）。

【决策】`ferrin-schema` 默认使用 `schemars` 的 draft-07 设置生成 Schema。依据：draft-07 的 `definitions` 与 `type` 数组是各供应商解析器普遍支持的结构（[ADR 0004](../04-decisions/2026-09-13-0004-schema-library-and-dialect.md)）。`SchemaSettings` 可由应用覆盖为 2020-12。

### 5.3 JSON 解析安全

【事实】JavaScript 运行时解析含 `__proto__`/`constructor.prototype` 键的 JSON 可能造成原型污染；Rust 的 `serde_json` 把对象解析为普通映射，不存在该风险，Ferrin 因此不需要专门的安全解析步骤。

【决策】Rust 无原型污染问题；`ferrin_schema::json::parse` 转而施加资源限制：最大嵌套深度（默认 128）与最大字节数（默认 64 MiB，供应商响应体另有 HTTP 层限制），超限返回 `JsonParseError`。依据：`serde_json` 默认递归限制为 128，显式配置便于在配置文档中说明。

## 6. 示例

```rust
#[derive(Debug, Deserialize, JsonSchema)]
struct Recipe {
    name: String,
    ingredients: Vec<String>,
    steps: Vec<String>,
}

let result = ferrin::generate_text(&model)
    .prompt("Generate a lasagna recipe.")
    .output(Output::<Recipe>::object())
    .await?;

println!("{} ingredients", result.output.ingredients.len());
```

流式：

```rust
let stream = ferrin::stream_text(&model)
    .prompt("Generate a lasagna recipe.")
    .output(Output::<Recipe>::object())
    .await?;

let mut partials = stream.partial_output_stream();
while let Some(partial) = partials.next().await {
    if let Some(recipe) = partial.typed {
        println!("complete so far: {}", recipe.name);
    }
}
```

## 7. 待验证

- 【事实】（PV-004）`schemars` draft-07 对 `Option<T>` 生成 `type: [T, "null"]`（原始类型）或 `anyOf: [$ref, {type: null}]`（引用类型），对枚举生成 `enum`/`oneOf`；适配器变换（OpenAI 严格模式的补全、Anthropic 的 `sanitize_json_schema`）按这些形状处理可空类型与枚举。详见[工具系统](06-tool-system.md)第 10 节。
- 【事实】（PV-008，`verification/pv008-partial-compare`，release 构建）`serde_json::Value` 深比较耗时：9 KiB 对象 16 µs、96 KiB 对象 135 µs、507 KiB 对象 675 µs；同一对象序列化后哈希的耗时更高（18 µs / 180 µs / 916 µs）；解析耗时是比较的 6–7 倍（93 µs / 936 µs / 4.7 ms）。
- 【决策】部分输出比较保持 `Value` 深比较，不改为文本哈希。依据：比较成本低于解析成本一个数量级，且哈希方案更慢；若未来出现热点，优化方向是增量解析而非比较方式。

【决策】动态 JSON Schema 校验使用 `$schema` 声明的方言，仅在未声明时默认采用 draft-07。必须执行方言专属约束，不能将其作为未知关键字静默忽略。

【决策】部分 JSON 修复跟踪对象键中的转义引号，并且只在 Unicode 标量完整时提交转义序列，包括代理对的两个部分。数值指数中的正号仍属于该数值，包括完整文档和截断前缀。前缀测试覆盖带符号指数、任意序列化键和显式 Unicode 转义，确保不完整的键和标量不会生成无效的修复 JSON。

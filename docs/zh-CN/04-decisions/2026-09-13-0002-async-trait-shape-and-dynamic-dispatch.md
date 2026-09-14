# 0002: 异步 trait 形态与动态分发

[English](../../04-decisions/2026-09-13-0002-async-trait-shape-and-dynamic-dispatch.md) | **简体中文**

- 状态：accepted
- 日期：2026-09-13
- 相关：[Provider 规范层](../01-architecture/04-provider-spec.md)、[总体架构](../01-architecture/01-overall-architecture.md)第 5 节

## 背景

【事实】中间件与注册表天然需要对象级组合：中间件包装一个模型并返回新的模型，注册表按字符串 ID 返回任意供应商的模型实例，二者都要求跨供应商类型的动态分发。

【事实】Rust 1.75 起 trait 方法可以直接返回 `impl Future<Output = T> + Send`（返回位置 `impl Trait`），无需 `async-trait` 宏；但含此类方法的 trait 不是对象安全的，`dyn Trait` 需要另行处理。

Rust 1.98 中带 RPITIT 方法的 trait 不是对象安全的。

## 决策

规范层 trait 使用 RPITIT（`impl Future + Send`）定义；为每个需要动态分发的 trait 提供对象安全的 `Dyn*` trait（方法返回 `BoxFuture`）与 blanket 实现；核心层统一持有 `Arc<dyn Dyn*>`。

## 依据

- 实现者获得原生 `async fn` 体验，无宏依赖。
- 装箱开销只发生在核心层边界，每次模型调用一次分配，相对网络往返可忽略。
- 与工作区编码规范一致（trait 方法显式声明 `impl Future + Send`，`Send` 约束可见）。

## 备选方案

- `#[async_trait]`：对象安全、简单，但每个方法装箱，且把 `Send` 约束隐藏在宏展开中。
- 直接以 `BoxFuture` 定义规范 trait：对象安全，但实现者需手写 `Box::pin(async move {...})`，且失去 `Send` 推断的便利。
- `dynosaur` 生成适配：【事实】（PV-001）0.3.1 满足 `Send + 'static` 约束（`verification/pv001-dynosaur`），但生成的是不定长结构体而非 trait 对象，需显式 `new_arc`/`new_box` 构造、泛型代码需 `?Sized`，并把一个 0.x 过程宏引入规范层。【决策】不采用，保留手写 `Dyn*` trait（见[总体架构](../01-architecture/01-overall-architecture.md)第 5 节）。

## 影响

- `ferrin-spec::dynamic` 模块包含约 12 个适配 trait 与实现；变更 trait 签名需同步修改。
- 公共 API 中的模型引用类型为 `Arc<dyn DynLanguageModel>` 等别名。

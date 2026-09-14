# 0007: Tokio 运行时与取消令牌

[English](../../04-decisions/2026-09-13-0007-tokio-runtime-and-cancellation.md) | **简体中文**

- 状态：accepted
- 日期：2026-09-13
- 相关：[并发、取消与超时](../01-architecture/16-concurrency-and-cancellation.md)

## 背景

【事实】生成循环需要把调用方的取消与多级超时合并后传递给模型调用、下载与工具执行；工具并发执行、下载并发、流级聚合都依赖异步任务与定时器。

【事实】Tokio 是 reqwest、hyper、tokio-tungstenite 与主流 MCP 实现共同依赖的运行时；Clippy 的 `await_holding_lock`/`await_holding_invalid_type` 可以在编译期禁止持锁跨 `.await`。

## 决策

1. Tokio 是唯一支持的运行时；库代码不隐式创建运行时，不使用 `block_on`。
2. 取消原语为 `tokio_util::sync::CancellationToken`，按调用 → 步骤 → 模型调用/工具/下载层级派生。
3. 后台任务通过 `JoinSet` 管理；`tokio::spawn` 被 lint 禁止。
4. 全部公共 Future/Stream 为 `Send`。

## 依据

- Tokio 是 reqwest、tokio-tungstenite 等依赖的共同基础，抽象运行时收益低、维护成本高。
- `CancellationToken` 支持层级派生，可表达“调用方取消或超时触发”的合并语义。
- `JoinSet` 保证任务随所有者取消，避免泄漏。

## 备选方案

- 运行时无关（`async-std`/`smol` 兼容）：需要抽象定时器与任务生成，且主要依赖仍绑定 Tokio。
- 使用 `futures::AbortHandle`：无层级派生，无法表达超时作用域。

## 影响

- 应用必须在 Tokio 运行时内调用 Ferrin。
- 需要区分取消原因（调用方取消或超时）的辅助状态。

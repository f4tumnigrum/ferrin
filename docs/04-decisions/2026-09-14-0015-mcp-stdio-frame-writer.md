# 0015: MCP stdio 传输以写入任务串行化帧

- 状态：accepted
- 日期：2026-09-14
- 相关：[MCP 集成](../01-architecture/15-mcp.md)第 5 节（PV-018 基线）与第 6 节；[编码规范](../03-engineering/03-coding-standards.md)；`clippy.toml` 的 `await-holding-invalid-types`

## 背景

MCP 集成文档第 5 节为 stdio 传输记录了实现基线：`tokio::process::Command` + `kill_on_drop(true)`，stdin 写入持锁以保证一帧（一行 JSON）原子写出，Windows 下以 `creation_flags` 抑制控制台窗口。实现 `ferrin-mcp` 时，工作区的 `clippy::await_holding_invalid_type`（`clippy.toml` 把 `tokio::sync::MutexGuard` 列为禁止跨 `await` 持有的类型）拒绝“持 `tokio::sync::Mutex<ChildStdin>` 的守卫跨 `write_all().await` 与 `flush().await`”这一写法；`std::sync::Mutex` 的守卫不能跨 `await`，也不能包住异步写入。按[架构决策记录流程](../03-engineering/07-adr-process.md)第 6 节记录对基线机制的修订。

## 决策

1. stdio 传输的 stdin 由**单一写入任务**独占（`JoinSet` 中的任务持有 `ChildStdin`），`send()` 把整帧文本与一个 `oneshot` 回执发送到无界 `mpsc` 通道，写入任务按到达顺序 `write_all` + `flush` 后回传 `io::Result`。
2. 原子性保证不变：任一时刻只有写入任务触碰 stdin，帧之间不会交错；`send()` 在收到回执后才返回，写入失败以 `McpError::Io` 传回调用方。
3. 关闭时先丢弃通道发送端（写入任务在排空队列后 `shutdown()` stdin 并退出），再 `start_kill()` 子进程并等待退出；读取任务在 stdout EOF 时同样关闭通道并投递 `TransportEvent::Closed`。
4. PV-018 的其余基线（`kill_on_drop(true)`、Windows 的 `creation_flags(CREATE_NO_WINDOW)`、命令与参数拒绝换行）不变，Windows 行为仍由 CI 的 `windows-2025` 作业验证。

## 依据

- 与“持锁写入”相比，写入任务不需要任何跨 `await` 的守卫，符合工作区对 tokio 互斥守卫的禁令，也避免了取消 `send()` 的 future 时把半写的帧留在管道里（写入任务不会被调用方的取消中断）。
- 通道回执保留了“调用方得知写入结果”的语义；无界通道不会阻塞投递方，背压由上层的请求超时承担。

## 备选方案

- `tokio::sync::Mutex<ChildStdin>` 持锁写入：被工作区 lint 拒绝；拒绝。
- `std::sync::Mutex` + 同步阻塞写入（`std::process::ChildStdin`）：阻塞运行时线程，且与 tokio 子进程 API 混用；拒绝。
- 每次 `send()` 先把帧拷入 `BytesMut` 再以 `try_lock` 自旋：仍需守卫跨 `await`；拒绝。

## 影响

- MCP 集成文档第 5 节的基线措辞按本 ADR 更正，第 6 节记录实现。
- 该模式适用于其他“多生产者写同一异步字节流”的场景（例如未来的 WebSocket 发送端）。

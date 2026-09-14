# 0016: 编码与序列化在异步任务内直接执行，不使用 `spawn_blocking`

- 状态：accepted
- 日期：2026-09-14
- 相关：[并发与取消](../01-architecture/16-concurrency-and-cancellation.md)第 7 节；[编码规范](../03-engineering/03-coding-standards.md)；`ferrin_core::limits`

## 背景

并发与取消文档第 7 节原先规定：大文件的 base64 编码与大型 JSON 序列化（> 1 MiB）在 `tokio::task::spawn_blocking` 中执行，阈值常量集中在 `ferrin_core::limits`。骨架阶段据此在 `limits.rs` 预留了 `BLOCKING_ENCODE_THRESHOLD_BYTES = 1 MiB`（带 `#[allow(dead_code)]`）。

【事实】截至 2026-09-14 全部 15 个 crate 实现完成，工作区中没有任何 `spawn_blocking` 或 `block_in_place` 调用：提示词转换（`ferrin-core::prompt::convert`）、文件与图像模态、供应商请求体构造都在调用方的异步任务内直接编码；该常量从未被读取。按[架构决策记录流程](../03-engineering/07-adr-process.md)第 6 节，记录对既有决策的修订，而不是补上一个没有实际需求的线程池路径。

## 决策

1. base64 编码、JSON 序列化与其他纯 CPU 转换在调用方的异步任务内直接执行；核心层与供应商 crate 不引入 `spawn_blocking`/`block_in_place`。
2. 删除 `ferrin_core::limits::BLOCKING_ENCODE_THRESHOLD_BYTES`；`limits` 只保留实际使用的通道容量与并发上限常量。
3. 并发与取消文档第 7 节改为记录本决策；若将来出现单次调用中 MB 级以上、可测量地阻塞运行时的编码工作，再以新的 ADR 引入阈值与阻塞线程路径，并附基准数据。

## 依据

- 阈值以下的负载占绝大多数：模型输入的文件通常远小于 1 MiB，且 `ferrin-core::limits` 对下载体积已有上限；对 MB 级数据 base64 编码耗时在毫秒量级，低于一次网络往返，阻塞线程切换带来的复杂度（阻塞线程池饱和、`JoinError` 处理、取消语义）没有对应收益。
- `spawn_blocking` 的任务不能被调用方的 `CancellationToken` 中断，与文档第 1 节的取消模型（`CancellationToken` 在 await 点生效）不一致；直接执行时取消语义与其他阶段相同。
- 工作区禁止 `tokio::spawn`（用 `JoinSet` 保证任务随所有者取消）；阻塞任务同样需要所有权约束，避免引入第二套任务生命周期规则。

## 备选方案

- 保留常量并在文件模态中接入 `spawn_blocking`：没有实测的阻塞问题，增加取消与错误处理路径；拒绝。
- 以 `block_in_place`：只在多线程运行时可用，当前线程运行时（`xtask`、测试）会 panic；拒绝。

## 后果

- `ferrin_core::limits` 缩减为三个常量；文档第 7 节的阈值描述删除。
- 若未来引入阻塞路径，需要 ADR、基准与取消语义说明。

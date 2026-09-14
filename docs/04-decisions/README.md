# 架构决策记录索引

流程见[架构决策记录流程](../03-engineering/07-adr-process.md)。

| 编号 | 标题 | 状态 | 日期 |
| --- | --- | --- | --- |
| [0001](2026-09-13-0001-workspace-and-crate-boundaries.md) | 工作区与 crate 边界 | accepted | 2026-09-13 |
| [0002](2026-09-13-0002-async-trait-shape-and-dynamic-dispatch.md) | 异步 trait 形态与动态分发 | accepted | 2026-09-13 |
| [0003](2026-09-13-0003-json-value-and-serialization.md) | JSON 值类型与序列化格式 | accepted | 2026-09-13 |
| [0004](2026-09-13-0004-schema-library-and-dialect.md) | Schema 库与方言 | accepted | 2026-09-13 |
| [0005](2026-09-13-0005-stream-result-delivery.md) | 流式结果交付形态 | accepted | 2026-09-13 |
| [0006](2026-09-13-0006-error-model.md) | 错误模型 | accepted | 2026-09-13 |
| [0007](2026-09-13-0007-tokio-runtime-and-cancellation.md) | Tokio 运行时与取消令牌 | accepted | 2026-09-13 |
| [0008](2026-09-13-0008-no-implicit-default-provider.md) | 不提供隐式默认供应商 | accepted | 2026-09-13 |
| [0009](2026-09-13-0009-http-transport-and-secure-url.md) | HTTP 传输抽象与安全 URL 策略 | accepted | 2026-09-13 |
| [0010](2026-09-13-0010-mcp-protocol-implementation.md) | MCP 协议自实现 | accepted | 2026-09-13 |
| [0011](2026-09-13-0011-spec-versioning-by-crate-version.md) | 以 crate 版本表达规范版本 | accepted | 2026-09-13 |
| [0012](2026-09-13-0012-tool-typing-strategy.md) | 工具类型化策略 | accepted | 2026-09-13 |
| [0013](2026-09-13-0013-core-implementation-revisions.md) | 核心层实现阶段对既有决策的修订 | accepted | 2026-09-13 |
| [0014](2026-09-13-0014-openai-compatible-model-families.md) | `ferrin-openai-compatible` 的模型族与 Responses 模式 | accepted | 2026-09-13 |
| [0015](2026-09-14-0015-mcp-stdio-frame-writer.md) | MCP stdio 传输以写入任务串行化帧 | accepted | 2026-09-14 |
| [0016](2026-09-14-0016-inline-encoding-no-spawn-blocking.md) | 编码与序列化在异步任务内直接执行，不使用 `spawn_blocking` | accepted | 2026-09-14 |
| [0017](2026-09-14-0017-apache-2-license-and-attribution.md) | 许可改为 Apache-2.0 单许可并署名派生代码 | accepted | 2026-09-14 |

## 编辑性修订

- 2026-09-14：ADR 0001–0014 的「背景」「依据」「备选方案」「影响」段落做了编辑性修订，删除对外部项目的引用，改以技术事实与 Ferrin 自身的约束表述；各 ADR 的决策内容、状态与日期未变。

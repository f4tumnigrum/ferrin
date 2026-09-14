# 项目定位与范围

[English](../../00-overview/01-project-scope.md) | **简体中文**

## 1. 定位

Ferrin 是一个 Rust 库形态的 AI SDK。调用方是直接依赖 Ferrin crate 的 Rust 应用（服务端程序、CLI、桌面应用后端、嵌入式代理进程）。Ferrin 提供：

- 与具体模型供应商解耦的统一调用接口（文本生成、流式生成、工具调用循环、结构化输出、嵌入、图像、语音、转写、重排、视频、文件与技能上传、批处理、实时会话）。
- 一套稳定的 Provider 规范，使供应商适配器可以独立实现、独立发布。
- 工具系统（本地工具、供应商执行工具、动态工具、MCP 工具），包含审批、修复、超时与遥测。
- 面向长期维护的工程基础设施：多 crate 工作区、统一 lint、fixture 测试、ADR 流程、语义化版本策略。

Ferrin 是独立实现的 Rust 库，设计阶段参考了 Vercel AI SDK 的公开源码与文档，部分代码由其移植而来（署名见仓库根目录的 `NOTICE` 与 README 的「致谢」，许可见 [ADR 0017](../04-decisions/2026-09-14-0017-apache-2-license-and-attribution.md)）。接口形态按 Rust 的所有权、类型系统与异步模型确定，不携带历史 API 命名、兼容层、`experimental_` 前缀迁移路径或多规范版本共存机制。

## 2. 功能范围

【决策】Ferrin 的目标功能范围覆盖下列能力域。依据：这些能力域是主流供应商 API（OpenAI、Anthropic、Google）与 MCP 协议共同暴露的功能面，合在一起构成一个完整、自洽的 SDK 表面；每个能力域的行为在各架构文档中以事实与决策条目定义。

| 能力域 | Ferrin 归属 crate |
| --- | --- |
| Provider 规范（12 类模型/资源接口） | `ferrin-spec` |
| 应用侧消息模型 | `ferrin-message` |
| 工具定义与执行 | `ferrin-tool`、`ferrin-core` |
| Schema 与 JSON 处理 | `ferrin-schema` |
| HTTP、SSE、重试分类、安全 URL | `ferrin-provider-util` |
| 文本生成循环、流式管线、结构化输出、Agent、中间件、注册表、遥测 | `ferrin-core` |
| 嵌入、图像、语音、转写、重排、视频、文件、技能、批处理、实时 | `ferrin-core` |
| MCP 客户端 | `ferrin-mcp` |
| OpenTelemetry 导出 | `ferrin-otel` |
| 测试辅助 | `ferrin-testing` |
| 供应商适配器 | `ferrin-openai`、`ferrin-anthropic`、`ferrin-openai-compatible`、`ferrin-google` |

## 3. 非目标

- 浏览器/前端 UI 状态管理（React hooks 一类的前端绑定）。Ferrin 只提供可序列化的流事件类型，供 Rust 服务向任意前端转发。
- 与任何现有前端消息协议的兼容层。Ferrin 定义自己的事件序列化格式（见[生成循环与流式管线](../01-architecture/07-generation-loop-and-streaming.md)）。
- 托管网关与隐式默认供应商（进程级全局默认模型解析）。Ferrin 不发起任何未显式配置的网络请求。
- 工作流序列化与恢复（把进行中的生成循环序列化到外部存储再恢复）。
- 沙箱执行环境的具体实现（Docker、远程沙箱）。Ferrin 只定义 `Sandbox` trait。
- 托管代理运行环境（harness）抽象。该层依赖特定的托管基础设施，Ferrin 不纳入。

## 4. 设计原则

【决策】以下原则是 Ferrin 的设计基线，针对长期维护的 Rust 库场景制定。依据：多 crate 工作区需要清晰的依赖方向（1、2），公共 API 一经发布即受语义化版本约束（3、4），库不应替应用做隐式选择（5），而文档只有在来源可追溯时才能作为实现依据（6）。

1. 适配器模式是骨架。应用代码只依赖 `ferrin-spec` 定义的 trait；供应商差异封装在适配器 crate 中，通过 `provider_options`/`provider_metadata` 透传。
2. 构件分离。消息模型、工具系统、Schema、HTTP 工具、核心循环分属不同 crate，允许单独依赖与替换。
3. 保守的公共 API 表面。每个 crate 显式导出；未导出即为私有；新增公共类型需在 API 参考中登记。
4. 三次法则。重复出现三次以上的模式才抽象为公共工具函数。
5. 显式优先。不使用全局可变状态承载配置；不使用布尔参数或裸 `Option` 参数表达模式选择；用枚举和命名方法。
6. 事实、决策、待验证分离。文档中每条陈述可追溯到源码或明确的技术依据。

## 5. 版本基线

- Rust 工具链：1.98.1（设计时最新官方稳定版，核实记录见[工具链与依赖版本](../03-engineering/01-toolchain-and-dependencies.md)）。
- 版本策略：`0.y.z` 阶段允许在次版本号中引入破坏性变更并在变更日志中标注；进入 `1.0` 后遵循语义化版本（见[版本与发布](../03-engineering/06-versioning-and-release.md)）。

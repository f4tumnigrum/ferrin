# 0025：Azure OpenAI 与 Voyage 供应商

[English](../../04-decisions/2026-09-17-0025-azure-and-voyage-providers.md) | **中文**

- 状态：proposed
- 日期：2026-09-17
- 相关：[crate 边界](../01-architecture/02-crates.md)、[供应商规范](../01-architecture/04-provider-spec.md)

## 背景

【事实】现有供应商没有 reranking 实现，也没有 Azure 专用认证与路由。本地 AI SDK 基线（`6c6c221`，`packages/voyage/src/reranking` 与 `packages/azure/src`）提供了这些接口的协议参考。

## 决策

【决策】增加 `ferrin-voyage` 的重排接口，以及 `ferrin-azure` 的 Responses、Chat、Completions、嵌入、图像、语音和非流式转写。门面增加可选 feature `voyage`、`azure`；版本号留待独立发布决策。

【决策】Voyage 复用规范与 HTTP 工具，支持文本和 JSON 文档（序列化并产生兼容性警告），拒绝响应中的非法索引；请求时从 `VOYAGE_API_KEY` 延迟读取凭据。

【决策】Azure 复用 `ferrin-openai` 模型，是 L3 依赖规则的明确例外，避免重复协议代码。私有认证传输与逐 deployment 配置支持 v1、旧版 deployment URL、API key 和逐请求调用的异步 Entra token provider。OpenAI 配置增加显式外部认证构造方式，不读取 `OPENAI_API_KEY`，不注入占位密钥。

【决策】Azure 只向配置的 origin 与 API 路径前缀附加凭据；下载仍使用既有安全 URL 策略，不向其他主机或同源无关路径发送认证。token provider 错误脱敏，不引入外部依赖。

## 替代方案

【决策】项目所有者选择 Azure，Bedrock 与 Vertex 留待以后。Voyage 本次只补重排，不扩展嵌入。拒绝复制全部 OpenAI 模型实现，以免请求、流式处理与安全修复分叉。

## 影响

【决策】[ADR 0026](2026-09-17-0026-reference-sdk-parity.md) 将默认凭据优先级修订为参考 SDK 行为：Azure 已有 Authorization 时跳过 Entra 获取，端点边界内的配置/调用请求头覆盖生成的凭据。Voyage 先解析 API 密钥再应用请求头覆盖。origin/路径隔离和脱敏仍为强制要求；此修订不改变本 ADR 的 proposed 状态。

【决策】fixture 测试仅验证协议转换，Azure 与 Voyage 的真实服务验证仍属于 PV-031。新 crate 同步许可证、NOTICE、文档、API 快照和发布顺序。ADR 保持 proposed，等待维护者评审。

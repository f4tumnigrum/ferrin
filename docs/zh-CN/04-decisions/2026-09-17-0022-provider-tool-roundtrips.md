# 0022：补齐供应商工具往返转换与调用者绑定

[English](../../04-decisions/2026-09-17-0022-provider-tool-roundtrips.md) | **中文**

- 状态：proposed
- 日期：2026-09-17
- 相关：[工具系统](../01-architecture/06-tool-system.md)、[OpenAI](../providers/openai.md)、[Anthropic](../providers/anthropic.md)

## 背景

【事实】 本地 AI SDK Responses 适配器映射托管程序、工具搜索和 shell 输出，并展开未声明的 `parallel` 包装。程序化调用及 Anthropic 代码执行工厂绑定供应商调用者并允许延迟结果。来源：2026-09-17 检查的 `packages/openai/src/responses/` 与 `packages/anthropic/src/tool/`。

## 决策

【决策】 使用 Ferrin 现有工具与调用者契约补齐上述路径。回放调用及结果时保留供应商条目 ID、程序指纹与调用者身份。托管 shell 与服务端工具搜索由供应商执行；客户端工具搜索与本地 shell 由应用执行。程序化调用及新版 Anthropic 代码执行工厂启用延迟结果，以支持客户端被调用工具在后续步骤完成。

【决策】 仅当未声明 `parallel`、所有嵌套接收者均为已声明函数且参数均为对象时展开包装。在供应商元数据中保留包装身份和子调用顺序，使存储回放发送一个原始调用及一个合并结果。无效包装和显式声明的包装保持普通调用。

【决策】 使用确定性协议回归检查生成、流式、回放和调用者准备。这些检查仅验证适配器行为；官方实时 API 验证仍属于 PV-031。

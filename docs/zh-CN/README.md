# Ferrin 设计文档

[English](../README.md) | **简体中文**

本目录是 Ferrin 的目标架构、技术设计与工程规范。设计文档与代码同仓维护：`docs/` 描述架构与规范，`crates/` 是其实现；各架构章节末尾的「实现记录」说明代码在何处细化了设计。使用者入口见仓库根目录的 [README](../../README.zh-CN.md)。

## 文档约定

- 本目录为独立中文版；英文主版本位于 `docs/`，内容冲突时以英文为准。代码、标识符、配置与示例使用英文。
- 每条陈述按来源分为三类，并在段落或条目前标注：
  - 事实（标注为 `【事实】`）：由供应商官方 API 文档、协议规范（MCP、RFC、OpenTelemetry 语义约定等）、依赖 crate 的文档或源码、`verification/` 原型或已记录的运行确认的行为或结构，附来源。
  - 决策（标注为 `【决策】`）：Ferrin 的设计决策，附技术依据。
  - 待验证（标注为 `【待验证】`）：尚未通过实验、原型或外部资料确认的事项，带 `PV-xxx` 编号并统一登记于[待验证事项汇总](05-appendix/02-pending-verification.md)；`scripts/docs_lint.py` 检查编号与登记。
- 写作与同步规则见[文档规范](03-engineering/09-documentation-standards.md)。

## 目录

### 00 概览

- [项目定位与范围](00-overview/01-project-scope.md)
- [术语表](00-overview/02-glossary.md)

### 01 架构与技术设计

- [总体架构](01-architecture/01-overall-architecture.md)
- [Crate 划分与职责](01-architecture/02-crates.md)
- [核心数据模型](01-architecture/03-core-data-model.md)
- [Provider 规范层](01-architecture/04-provider-spec.md)
- [Prompt 标准化与消息转换](01-architecture/05-prompt-conversion.md)
- [工具系统](01-architecture/06-tool-system.md)
- [生成循环与流式管线](01-architecture/07-generation-loop-and-streaming.md)
- [结构化输出](01-architecture/08-structured-output.md)
- [Agent](01-architecture/09-agent.md)
- [中间件与注册表](01-architecture/10-middleware-and-registry.md)
- [其他模态与资源接口](01-architecture/11-other-modalities.md)
- [错误模型](01-architecture/12-error-model.md)
- [可观测性](01-architecture/13-observability.md)
- [HTTP 传输与安全](01-architecture/14-http-and-security.md)
- [MCP 集成](01-architecture/15-mcp.md)
- [并发、取消与超时](01-architecture/16-concurrency-and-cancellation.md)
- [Provider 适配器实现指南](01-architecture/17-provider-implementation-guide.md)
- [策略化工具审批](01-architecture/18-policy-approval.md)

### 02 公共 API

- [API 设计原则](02-api/01-api-design-principles.md)
- [API 参考与示例](02-api/02-api-reference.md)

### 03 工程规范

- [工具链与依赖版本](03-engineering/01-toolchain-and-dependencies.md)
- [工作区布局](03-engineering/02-workspace-layout.md)
- [编码规范](03-engineering/03-coding-standards.md)
- [测试规范](03-engineering/04-testing.md)
- [CI 与质量门禁](03-engineering/05-ci-and-quality-gates.md)
- [版本与发布](03-engineering/06-versioning-and-release.md)
- [架构决策记录流程](03-engineering/07-adr-process.md)
- [安全规范](03-engineering/08-security-practices.md)
- [文档规范](03-engineering/09-documentation-standards.md)

### 04 架构决策记录

- [ADR 索引](04-decisions/README.md)

### 05 附录

- [核心行为清单](05-appendix/01-core-behaviors.md)
- [待验证事项汇总](05-appendix/02-pending-verification.md)

### 供应商与生成文件

- 供应商能力矩阵、设置与选项：[OpenAI](providers/openai.md)、[Anthropic](providers/anthropic.md)、[Google](providers/google.md)、[OpenAI 兼容端点](providers/openai-compatible.md)
- [`../api/`](../api/)（两版共用）：`cargo xtask api-snapshot` 生成的各 crate 公共 API 摘要（JSON），CI 校验其与代码一致

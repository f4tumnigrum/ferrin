# 0010: MCP 协议自实现

[English](../../04-decisions/2026-09-13-0010-mcp-protocol-implementation.md) | **简体中文**

- 状态：accepted
- 日期：2026-09-13
- 相关：[MCP 集成](../01-architecture/15-mcp.md)

## 背景

【事实】MCP 客户端需要覆盖 JSON-RPC 消息、Streamable HTTP 与 SSE 传输、协议版本协商、OAuth、诱导（elicitation）、MCP Apps 与请求头绑定等能力（MCP 规范 2026-07-28 与 2025-11-25）。

【事实】官方 Rust SDK `rmcp` 提供客户端、传输与协议类型实现，设计时（2026-09-13）crates.io 最新版本为 3.3.0。

## 决策

`ferrin-mcp` 自行实现 MCP 所需的 JSON-RPC 与协议子集、三种传输与 OAuth，不依赖 `rmcp`。

## 依据

- 所需协议子集有限，且必须与 Ferrin 的 Schema、工具输出、审批、指纹机制紧密耦合。
- 传输层细节（重定向默认拒绝、会话过期回调、恢复令牌、`x-mcp-header` 绑定、版本探测）需要完全控制。
- 避免第三方 SDK 类型进入公共 API 与版本联动。

## 备选方案

- 基于 `rmcp` 3.3.0 封装：协议更新由上游维护，但需大量适配层，且传输行为受上游约束。

## 影响

- 需跟踪 MCP 规范版本更新；`protocol/versions.rs` 集中维护支持列表。
- 【事实】（PV-017）已对照 2026-07-28 规范：该版本移除会话、GET 流、`Last-Event-ID` 恢复与服务端请求，改为逐请求元数据 + `Mcp-Method`/`Mcp-Name`/`Mcp-Param-*` 头与 MRTR；2025-11-25 保留旧语义。`ferrin-mcp` 按双代客户端实现（见 [MCP](../01-architecture/15-mcp.md) 第 2.2.1 节），这加强了自实现的理由：需要在一个客户端内同时精确控制两代传输行为。
- stdio 传输的子进程管理与 Windows 管道行为需要自行验证（PV-018，已由 Windows CI 作业关闭）。

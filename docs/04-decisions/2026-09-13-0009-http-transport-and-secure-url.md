# 0009: HTTP 传输抽象与安全 URL 策略

- 状态：accepted
- 日期：2026-09-13
- 相关：[HTTP 传输与安全](../01-architecture/14-http-and-security.md)、[安全规范](../03-engineering/08-security-practices.md)

## 背景

【事实】SDK 会按应用传入的 URL 下载文件并连接 MCP 端点，若不加限制即构成 SSRF（服务端请求伪造）面：需要限制 HTTPS、拒绝私网与本地地址、对重定向逐跳重校验、固定 DNS 解析结果、限制下载大小，并以 lint 保证所有出站请求经由受审计入口。

【事实】把传输抽象为 trait（默认实现基于 reqwest）后，测试可以注入录制回放传输而不依赖网络，应用也可以替换为代理或特殊运行时的实现。

## 决策

1. 定义 `HttpTransport` trait，默认实现基于 reqwest 0.13.5 + rustls；禁用自动重定向。
2. 自实现 SSE 解码器。
3. `secure_url` 模块实现 `UrlPolicy`、`validate_url`、`fetch`（DNS 解析后全部地址校验、`resolve_to_addrs` 固定、手动重定向、体积上限）。
4. Clippy `disallowed-methods` 禁止在受审计模块外直接使用 reqwest 与 `std::env::var`。

## 依据

- trait 形态支持测试注入、代理与自定义运行时。
- 自实现 SSE 便于加入分片时间戳与体积限制，且避免依赖长期未更新的 crate。
- Clippy `disallowed-methods` 在编译期强制，比评审约定可靠。

## 备选方案

- 直接暴露 `reqwest::Client`：把第三方类型固定进公共 API，测试与替换困难。
- 使用 `eventsource-stream`/`reqwest-eventsource`：缺少所需的计时与限制钩子。

## 影响

- 【事实】（PV-015）reqwest 0.13.5 保留 `resolve_to_addrs` 与 `redirect::Policy::none()`，每目标客户端构建成本约 58 µs（`verification/pv015-reqwest`）；feature 名为 `rustls`（0.13 起），默认证书校验器为 `rustls-platform-verifier`。
- 供应商 crate 只能通过 `ferrin_provider_util::http` 发请求。

# 安全规范

## 1. 威胁模型

Ferrin 处理的不可信输入来源：

| 来源 | 内容 | 风险 |
| --- | --- | --- |
| 模型输出 | 工具调用参数、URL、JSON | 注入恶意 URL 触发服务端请求伪造（SSRF）；超大或畸形 JSON 导致资源耗尽 |
| 供应商响应 | 响应体、SSE 分片、响应头 | 畸形数据导致解析错误或内存膨胀 |
| 应用持久化的消息历史 | 审批响应、工具结果 | 篡改审批绕过策略 |
| MCP 服务器 | 工具定义、工具结果、诱导请求 | 工具定义漂移、提示注入（由应用层处理）、恶意资源 |
| 环境变量与配置 | 密钥、base URL | 泄漏、指向恶意端点 |

## 2. 网络访问

【决策】所有出站下载与 MCP 端点遵循统一的安全 URL 规则：仅 HTTPS、拒绝私网与本地地址、重定向逐跳重校验、DNS 固定、100 MiB 下载上限、以 Clippy `disallowed-methods` 强制经由受审计入口。依据：这些规则共同封堵 SSRF（服务端请求伪造）与 DNS 重绑定；实现见[HTTP 传输与安全](../01-architecture/14-http-and-security.md)。

- 所有出站请求经 `ferrin_provider_util::http`；Clippy `disallowed-methods` 阻止直接使用 reqwest。
- 来自模型或消息的 URL 只能通过 `secure_url::fetch` 下载，默认策略见 [HTTP 传输与安全](../01-architecture/14-http-and-security.md)第 8 节。
- 供应商 base URL 来自应用配置或环境变量，构造时校验为绝对 URL、仅 `http`/`https`、无凭据部分；`http` 仅允许显式配置（本地代理场景）并记录 `warn` 日志。
- 重定向不自动跟随。
- 默认 TLS 为 rustls + webpki-roots；可选系统证书校验器。

## 3. 密钥处理

- API 密钥、审批签名密钥、OAuth 令牌使用 `secrecy` 类型；`Debug`/`Display` 遮蔽。
- 密钥只在构造请求头的瞬间 `expose_secret()`；不写入日志、错误消息、遥测、fixture。
- 错误中的 `Headers` 在 `Debug` 输出时对敏感头遮蔽（`authorization`、`x-api-key`、`cookie`、`set-cookie`、`proxy-authorization`，以及供应商 crate 注册的额外头名）。
- fixture 录制脚本白名单保留响应头，写入前扫描密钥模式。
- 环境变量读取集中在 `ferrin_provider_util::settings`，便于审计。

## 4. 工具执行

- 工具输入在执行前按 Schema 校验；无 Rust 类型的动态工具（MCP）在启用 `json-schema-validation` 时按 JSON Schema 校验。
- 审批机制：需要审批的工具默认不执行；审批响应可选 HMAC-SHA256 签名，签名校验使用常量时间比较；输入重新校验、策略重新解析（实现见[工具系统](../01-architecture/06-tool-system.md)）。
- 工具指纹：应用可在跨请求场景比较工具集指纹，检测定义漂移。
- 工具执行受超时与取消约束；工具错误不终止循环但会反馈给模型。
- `Sandbox` trait 的本地进程实现明确标注不提供隔离，仅用于测试。

## 5. 资源限制

| 资源 | 默认上限 | 位置 |
| --- | --- | --- |
| 下载体积 | 100 MiB | `UrlPolicy::max_body_bytes` |
| 非流式响应体 | 64 MiB | `ferrin_provider_util::http::limits` |
| 单个 SSE 事件 | 16 MiB | `SseDecoder` |
| JSON 嵌套深度 | 128 | `ferrin_schema::json` |
| 工具输入 JSON | 4 MiB | `parse_tool_call` |
| 工具结果注入通道 | 64 项 | 流式管线 |

超限行为：中止当前请求并返回 `ApiCallError`/`JsonParseError`（可重试性为 `false`）。

【决策】上限为常量并可通过构建器的 `limits(Limits)` 覆盖；不通过环境变量配置。依据：库行为不应因进程环境隐式变化。

## 6. 依赖安全

- `cargo deny` advisories 每日运行；高危漏洞 72 小时内发布补丁。
- `unsafe_code = "forbid"`。
- 依赖来源仅 crates.io。
- 发布产物由 CI 从 tag 构建；维护者本地不执行 `cargo publish`（发布令牌仅在 CI）。

## 7. 漏洞报告

- `SECURITY.md` 说明私下报告渠道与响应时限（确认 3 个工作日，修复目标 30 天）。
- 修复发布后在变更日志 `Security` 段说明并申请 RUSTSEC 公告（若影响下游）。

## 8. 待验证

- 【事实】（PV-022，`verification/pv022-crypto`）`hmac` 0.13 + `sha2` 0.11 + `subtle` 2.6 + `secrecy` 0.10 可共同编译：`Hmac::<Sha256>::new_from_slice` 需引入 `hmac::KeyInit` trait；密钥以 `SecretBox<[u8]>`（`SecretBox::from(Box<[u8]>)`）保存，`expose_secret()` 直接传入；验证使用 `Mac::verify_slice`（常量时间），预计算摘要比较使用 `subtle::ConstantTimeEq::ct_eq`。
- 【决策】（PV-028，`verification/pv028-ipv4-mapped`）私网判定前统一把 IPv4 映射 IPv6 地址规范化为 IPv4（`Ipv6Addr::to_ipv4_mapped()`），使结果不依赖平台解析器的表示；macOS 上 `lookup_host("localhost:443")` 返回 `[::1]:443, 127.0.0.1:443`。【事实】（PV-028，已关闭）Windows 运行器上的表示由 CI 的 `windows-2025` 作业记录：2026-09-14 `ci.yml` run 34798946529 中 `verification/pv028-ipv4-mapped` 的 `local_resolver_representation` 输出 `localhost -> [[::1]:443, 127.0.0.1:443] (os = windows)`，与 macOS 相同（IPv6 环回与 IPv4 环回各一条，无 IPv4 映射形式）；规范化逻辑保留，因为其他解析器配置仍可能返回 `::ffff:` 形式。

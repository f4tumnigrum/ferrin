# 0020：策略化工具审批 crate 与内嵌 Rego 引擎

[English](../../04-decisions/2026-09-15-0020-policy-based-tool-approval.md) | **简体中文**

- 状态：accepted
- 日期：2026-09-15
- 关联：[工具系统](../01-architecture/06-tool-system.md)、[策略化工具审批](../01-architecture/18-policy-approval.md)、[ADR 0017](2026-09-14-0017-apache-2-license-and-attribution.md)

## 背景

【事实】Ferrin 通过 `ApprovalPolicy` trait 判定工具审批（工具系统第 4.1 节）；至今每个决策都以 Rust 写在应用中。运行 Open Policy Agent 的组织把授权表达为按 JSON 输入求值的 Rego 策略，或经 OPA 的 REST Data API，或内嵌求值器。

【事实】Vercel AI SDK 提供一个策略包，含 HTTP 与 WASM 策略客户端、决策归一化、影子模式、面向 MCP 工具的全覆盖包装与能力中间件（2026-09-15 阅读其仓库）。Ferrin 此前没有对应物；2026-09-15 的差距分析列出了这一项。

## 决策

【决策】新增 crate `ferrin-policy`（L5 层，依赖 `ferrin-core`、`ferrin-spec`、`ferrin-provider-util`），提供 `PolicyClient` trait、`PolicyDecision` 归一化、`policy_approval`（一个 `ApprovalPolicy`）、`shadow`、`with_default` 与 `capability_middleware`。门面在 `policy` 与 `policy-rego` 下暴露它。

【决策】两种客户端：经 `ferrin_provider_util::http` 与安全 URL 策略访问 OPA REST Data API 的 `HttpPolicyClient`，以及 crate feature `rego` 下用 `regorus` 0.12 进程内求值 Rego 的 `RegoPolicyClient`。

【决策】默认失败关闭：评估错误拒绝调用（或清空工具），未识别的决策文档拒绝，`not-applicable`（或未定义规则）回落到工具自身的 `needs_approval`。失败开放需显式选择（`FailureMode::FallThrough`）。除决策对象与旧式 `allow` 形式外，也接受裸布尔值作为决策。

## 依据与备选方案

【决策】独立 crate 而非 `ferrin-core` 模块：HTTP 客户端、URL 策略管线，尤其是 Rego 解释器（默认 feature 下约四十个间接 crate）不应由生成循环的每个用户承担，且该 crate 可按自身节奏演进。

【决策】选 `regorus` 而非其他方案：OPA 的 WASM 包需要 `wasmtime` 之类的 WebAssembly 运行时（依赖极重，且策略必须先用 `opa` 工具预编译）；调用 `opa` 二进制意味着进程管理与外部安装；`regorus` 是 Microsoft 的纯 Rust 解释器，许可宽松（`MIT AND Apache-2.0 AND BSD-3-Clause`，均在 deny 允许列表内），引擎实现 `Clone` 可无锁并发求值，并通过 `full-opa` feature 覆盖 OPA 内建函数。

【决策】单 crate 加 feature 而非 `ferrin-policy-rego` crate：HTTP 与 Rego 客户端共用路径归一化、决策格式与审批适配器，可选依赖限于 `rego.rs`。

【决策】不适用回落而非放行：没有针对某工具规则的策略不得覆盖工具作者的 `needs_approval`；需要全覆盖决策的部署使用 `with_default`。

## 影响

【决策】工作区共 16 个 crate；`ferrin-policy` 在 `ferrin-core` 之后、与 `ferrin-otel`、`ferrin-testing` 并列发布。它以工作区版本 0.1.1 起步，变更日志只有 `Unreleased` 段，尚无已发布版本。

【决策】启用 `rego` 后，`regorus` 向依赖图加入 `anyhow`、`lazy_static`、`num-bigint`、`spin` 与第二个 `jsonschema` 主版本（0.49）；这些仅为间接依赖，本 crate 自身不使用 `anyhow` 或 `lazy_static`，重复主版本按工具链文档记录为 `multiple-versions` 警告。`rego` feature 的 Windows MSVC 构建依赖 Spectre 缓解版 CRT 库（PV-032）。

【决策】决策格式与影子、能力模式源自 Vercel AI SDK；`ferrin-policy` 加入根 `NOTICE` 的署名列表，并按 ADR 0017 带有 `# Attribution` 段。

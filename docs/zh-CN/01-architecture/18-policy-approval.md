# 策略化工具审批

[English](../../01-architecture/18-policy-approval.md) | **简体中文**

由 `ferrin-policy` 实现（[ADR 0020](../04-decisions/2026-09-15-0020-policy-based-tool-approval.md)）。该 crate 把[工具系统](06-tool-system.md)第 4 节的 `ApprovalPolicy` 契约接到遵循 Open Policy Agent 约定的策略引擎上：JSON 输入、规则路径、JSON 决策。

## 1. 范围与定位

【决策】当团队需要跨服务共用一套规则、审计每一次决策、或让规则变更流程独立于部署时，审批决策应从应用代码中移出。`ferrin-policy` 通过 `PolicyClient` 评估这类规则并把决策映射为 Ferrin 的审批状态；它不定义自己的策略语言。

【决策】该 crate 位于 L5 层，与 `ferrin-otel` 并列：依赖 `ferrin-core`（审批与中间件 trait）、`ferrin-spec` 与 `ferrin-provider-util`（HTTP 传输、安全 URL 策略）。门面通过 feature `policy`（`ferrin::policy`）与 `policy-rego`（额外启用内嵌 Rego 引擎）暴露它。依据：HTTP 客户端与 Rego 解释器不应成为每个 `ferrin-core` 用户的依赖。

## 2. 策略客户端

### 2.1 接口

【决策】客户端以输入评估某个路径并返回原始决策文档；策略未产生值时返回 `null`（而非错误）。

```rust
pub trait PolicyClient: Send + Sync + 'static {
    fn evaluate<'a>(&'a self, path: &'a str, input: JsonValue)
        -> BoxFuture<'a, Result<JsonValue, PolicyError>>;
}
```

`Arc<T>` 转发到 `T`；`policy_client(|path, input| ..)` 把同步闭包适配为客户端，用于测试与静态规则。`PolicyError` 为 `thiserror` 枚举（`InvalidPath`、`InvalidUrl`、`InvalidInput`、`Transport`、`Status`、`InvalidResponse`、`Engine`）；审批策略不会把它抛给生成循环（第 4 节）。

【决策】路径同时接受 OPA REST 形式（`ferrin/tools/decision`）与 Rego 形式（`ferrin.tools.decision`），可带前导 `/` 或 `data` 段；拒绝空段与空白字符。依据：同一策略字符串可同时用于两种客户端。

### 2.2 HTTP 客户端（OPA REST Data API）

【事实】OPA Data API 通过 `POST /v1/data/{path}`、请求体 `{"input": <document>}` 带输入评估文档，响应为 `{"result": <value>}`；文档未定义时 `result` 成员缺失（来源：OPA 文档，REST API，Data API）。

【决策】`HttpPolicyClient::builder(base_url)` 经 `ferrin_provider_util::http`（`HttpTransport`，默认共享 `reqwest` 传输或注入的传输）发送该请求：若配置头未设置则补 `content-type` 与 `accept` 为 `application/json`，附加 `ferrin-policy/<version>` user-agent 后缀，可选的单次请求超时，默认 1 MiB 响应上限，以及用于认证的配置头。`base_url` 可带路径前缀，`/v1/data/<segments>` 追加其后。`result` 缺失时返回 `null`；非成功状态成为 `PolicyError::Status`（附最多 1 KiB 响应体）；非对象响应体为 `InvalidResponse`。

【决策】每次评估都按[HTTP 传输与安全](14-http-and-security.md)用 `UrlPolicy` 校验服务器 URL，解析并固定地址。默认策略为严格策略（HTTPS、公网）；本机 sidecar 需要 `UrlPolicy::new().allow_http().allow_private_networks()`。依据：与 MCP 端点相同的默认值，放宽必须是运维方的显式决定。

### 2.3 内嵌 Rego 客户端（feature `rego`）

【事实】`regorus` 0.12.0（2026-09-15 crates.io `max_stable_version`；许可 `MIT AND Apache-2.0 AND BSD-3-Clause`；未声明 `rust-version`）提供 `Engine::new()`、`add_policy(path: String, rego: String) -> Result<String>`（返回 `data.<package>` 路径）、`add_data(Value)`、`set_input(Value)` 与 `eval_rule(String) -> Result<Value>`；`Engine` 实现 `Clone`，`Value: From<serde_json::Value> + Serialize`，未定义规则求值为 `Value::Undefined`。默认 feature 为 `full-opa`、`arc`、`rvm`（来源：0.12.0 的 crate 源码）。

【决策】`RegoPolicyClient::builder().policy(name, source).data(json).build()` 一次性解析模块并合并数据文档。每次评估克隆已准备好的引擎、设置输入并求值 `data.<path>`，因此客户端无锁即为 `Sync`。`Value::Undefined` 变为 `null`（不适用）；不存在的规则路径为 `PolicyError::Engine` 错误，审批策略将其转为拒绝。依据：未定义规则是 Rego 表达“无意见”的正常方式，而路径拼写错误是配置错误，不得静默放行调用。

【事实】（PV-032，2026-09-16 验证）[CI run 35032650222](https://github.com/f4tumnigrum/ferrin/actions/runs/35032650222) 的 `test (windows-2025)` 作业在 `7f950ad` 上通过包含 Rego 客户端的全 feature 构建与测试。`regorus` 的 MSVC 构建仍要求 Spectre 缓解版 CRT 库；本结论仅验证该托管运行器，不代表任意 Windows 安装环境。

## 3. 决策文档

【决策】`PolicyDecision::normalize(raw)` 接受下列形式，其余一律视为拒绝并附原因 `unrecognized policy decision`，使损坏或误路由的策略失败关闭：

| 原始文档 | 决策 |
| --- | --- |
| `null`（未定义规则、缺失 `result`） | `NotApplicable` |
| `true` / `false` | `Allow` / `Deny` |
| `{"decision": "allow" \| "deny" \| "requires-approval" \| "not-applicable", "reason"?}` | 对应决策及原因 |
| `{"allow": bool, "reason"?}`（旧形式） | `Allow` / `Deny` |
| 其他任何值，包括未知的 `decision` 字符串 | 附未识别原因的 `Deny` |

【决策】`into_approval` 把 `Allow` 映射为 `Approved`、`Deny` 为 `Denied`、`RequiresApproval` 为 `UserApproval`（保留原因），`NotApplicable` 为 `None`，使审批策略回落到工具自身的 `needs_approval`。依据：没有针对某工具规则的策略不应覆盖工具作者的声明。

【决策】裸布尔形式是 Ferrin 的扩展：`default allow := false` 加 `allow if { .. }` 是最常见的 Rego 写法，含义没有歧义。

## 4. 审批策略

【决策】`policy_approval(client, path)` 实现 `ApprovalPolicy`。默认输入为

```json
{
  "tool": { "name": "..", "tool_call_id": "..", "dynamic": false, "provider_executed": false, "invalid": false },
  "input": <解析后的工具输入>,
  "messages": [<本步骤的消息>],
  "tools_context": <工具上下文或 null>
}
```

`to_input(|call, ctx| ..)` 可替换它，例如去掉消息。评估错误以原因 `policy evaluation failed` 拒绝调用；`on_error(FailureMode::FallThrough)` 则改为返回 `None`。决策以 `debug` 级别、失败以 `warn` 级别记录日志，只含工具名、路径和决策类型，不包含原因或错误载荷。

【决策】`with_default(policy, status)` 对内层策略未决定的调用返回 `status`，使没有 `needs_approval` 声明的工具（例如从 MCP 服务器桥接的工具）不会静默执行。`shadow(policy)` 评估内层策略，通过 `on_decision(|call, status| ..)` 上报每个决策，在设置 `enforcement(Enforcement::Enforce)` 之前一律返回 `None`；上线时从观察切换到执行无需改动接线。

针对默认输入的示例策略：

```rego
package ferrin.tools

import rego.v1

default decision := {"decision": "not-applicable"}

decision := {"decision": "deny", "reason": "protected path"} if {
    input.tool.name == "delete_file"
    startswith(input.input.path, "/tmp/")
}

decision := {"decision": "requires-approval"} if {
    input.tool.name == "delete_file"
    not startswith(input.input.path, "/tmp/")
}
```

## 5. 能力中间件

【决策】`capability_middleware(client, path)` 是一个 `LanguageModelMiddleware`，其 `transform_params` 把 `CallOptions::tools` 限制为策略返回的允许列表：工具名数组或 `{"tools": [..]}`。没有工具的调用不评估。默认输入为 `{"model": {"provider", "model_id"}, "call": "generate" | "stream", "tools": [{"name", "provider_defined"}], "tool_choice"}`，`to_input` 可替换。评估失败与未识别文档会移除全部工具（失败关闭），除非 `on_error(FailureMode::FallThrough)` 保留它们。强制指向已移除工具、或在无工具时要求必须调用工具的 `tool_choice` 会被清除，以免供应商拒绝请求。

## 6. 安全

- 【决策】默认审批输入包含本步骤的消息，策略服务器因此能看到 prompt 与工具输出。不得共享这些内容的部署应用 `to_input` 只发送工具名与输入。
- 【决策】HTTP 客户端的认证头通过 `Headers` 配置，在 `Debug` 输出中以掩码显示；本 crate 不记录任何头、输入或响应。
- 【决策】所有失败路径默认拒绝（审批）或清空工具列表（能力）；失败开放需通过 `FailureMode::FallThrough` 显式选择。
- 【决策】Rego 评估在调用方任务内同步执行；策略是运维方提供的代码，引擎的执行与策略体积限制保持 `regorus` 默认值。

## 7. 待验证与验收项

- 【决策】`ferrin-policy` 的最低覆盖（`tests/suite/`）：第 3 节归一化表格及全部被拒形式；默认输入文档与 `to_input` 覆盖；评估错误时的拒绝与回落；`with_default`；两种执行模式下的 `shadow`；两种允许列表形式的能力过滤、失败关闭与回落、`tool_choice` 清理、无工具时不评估；对 `FixtureServer` 的 HTTP 线路格式（两种分隔符的路径、请求体、头、user agent、缺失 `result`、非成功状态、非 JSON 与非对象响应体、非法路径、默认 URL 策略拒绝本机 HTTP 服务器、基路径前缀）；Rego 客户端（输入与数据、未定义规则、缺失规则、非法策略）及其经 `policy_approval` 的使用；经 `generate_text` 与 `MockLanguageModel` 观察到的拒绝、放行与需人工审批三种决策。
- 【事实】PV-032 已由第 2.3 节记录的 Windows CI 构建与测试关闭。

## 8. 实现记录（2026-09-15）

- 【事实】`crates/ferrin-policy/src/`：`client.rs`、`decision.rs`、`approval.rs`（`policy_approval`、`with_default`、`FailureMode`、`default_input`）、`shadow.rs`、`capability.rs`、`http.rs`、`rego.rs`（feature `rego`）、`path.rs`、`error.rs`。测试：`tests/suite/{decision,approval,shadow,capability,http,rego}.rs`，按第 7 节清单。
- 【事实】门面在 `policy` 下把该 crate re-export 为 `ferrin::policy`；`policy-rego` 转发 `ferrin-policy/rego`。

【决策】 能力过滤通过 core 中间件工具约束同时约束执行和工具选择校验。诊断信息不输出决策原因、错误载荷或服务器 URL 凭据；显式决策回调仍是应用自行控制的审计接口。

# Azure OpenAI（`ferrin-azure`）

[English](../../providers/azure.md) | **中文**

【决策】`ferrin-azure` 复用 OpenAI 协议实现并提供 Azure 路由与认证（[ADR 0025](../04-decisions/2026-09-17-0025-azure-and-voyage-providers.md)）。使用门面的 `azure` feature，或直接依赖该 crate。

## 能力与配置

【事实】`create_azure(AzureSettings)` 返回 `AzureProvider`，`responses`、`chat`、`completion`、`embedding`、`image`、`speech` 和 `transcription` 接收 deployment 名称；`Provider::language_model` 使用 Responses，`tools()` 暴露 OpenAI 工具工厂。来源：`crates/providers/ferrin-azure/src/provider.rs`。

【决策】选项和响应 metadata 使用 `openai` 键，模型标识使用 `azure.<family>`。和参考 Azure 适配器一样，模型能力规则依据 deployment 名称推断；应用需选择适合模型采样与图像限制的名称，SDK 不向 Azure 查询底层模型。Azure DeepSeek、realtime、文件、skills 与 batch 不属于本适配器范围。

【事实】凭据在请求时读取：显式 `api_key` 优先于 `AZURE_API_KEY`；也可用 `token_provider` 逐请求获取 Entra token，两种显式方式互斥。`resource_name` 或 `AZURE_RESOURCE_NAME` 构造 `https://<resource>.openai.azure.com/openai`，`base_url` 可覆盖。来源：`settings.rs`、`provider.rs`、`transport.rs`。

【决策】`AzureUrlMode::V1` 为未带版本的 Azure 主机补 `/v1`，保留完整 `/openai/v1` 和自定义网关 URL，并识别 Foundry project 路径。`Deployment` 使用 `/deployments/<deployment>` 和 `api-version`，默认版本为 `v1`；旧接口应配置适用的日期版本。非法 deployment 在 HTTP 前拒绝。

【决策】凭据限定于配置的 origin 和 API 路径前缀；跨域、同源相邻路径、URL 凭据、解码后的路径穿越、反斜线、控制字符和有歧义的双重编码都不会获得 Azure 凭据。保留原下载 URL 安全验证；专用认证覆盖调用级凭据头，错误与调试输出不暴露凭据。

【决策】token 获取支持取消和超时，其耗时从底层响应体剩余期限中扣除。HTTP 转写使用独立包装，其他 crate 开启 OpenAI realtime feature 不会意外启用 Azure WebSocket 路径。

## 示例

```rust,no_run
use ferrin::azure::{AzureSettings, create_azure};
use ferrin::prelude::*;

# async fn example() -> Result<(), Box<dyn std::error::Error>> {
let azure = create_azure(AzureSettings::new("my-resource"))?;
let response = generate_text(azure.responses("my-deployment"))
    .prompt("Explain ownership in Rust.")
    .await?;
# Ok(())
# }
```

## 验证

【事实】`crates/providers/ferrin-azure/tests/suite/` 验证 Responses 普通和流式生成、嵌入、URL 模式与 Foundry、逐请求 Entra 刷新、认证冲突、非法输入、错误脱敏、取消、凭据范围、DNS 固定地址传递和总超时计算。这些是 fixture 与注入传输测试，不代表真实服务验证。

【待验证】(PV-031) 录制 Azure 真实响应并对照 fixture；deployment 能力推断与各租户接口可用性尚未通过本地测试验证。

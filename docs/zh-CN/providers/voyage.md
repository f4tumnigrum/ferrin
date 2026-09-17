# Voyage

[English](../../providers/voyage.md) | **中文**

[决策] `ferrin-voyage` 实现 Voyage 非流式 rerank 端点。`create_voyage(VoyageSettings)` 返回 `VoyageProvider`；`reranking(model_id)` 创建具体模型，`Provider::reranking_model` 提供供 `rerank` 使用的动态模型。其他模型类别不受支持。参见 [ADR 0025](../04-decisions/2026-09-17-0025-azure-and-voyage-providers.md)。

## 配置

[决策] 默认端点为 `https://api.voyageai.com/v1`。配置支持基础 URL、API 密钥、自定义供应商名称、额外请求头和 HTTP 传输。密钥延迟从 `VOYAGE_API_KEY` 读取，显式密钥或 Authorization 请求头优先。单次调用请求头覆盖配置请求头；请求在 User-Agent 中附加 crate 版本。基础 URL 必须使用 HTTP(S)，且不能包含凭据、查询参数或片段；HTTP 支持显式配置的本地测试端点。

## 重排

[事实] 请求使用 `model`、`query`、`documents`、`top_k`、`return_documents` 和 `truncation`。响应的 `data` 条目包含 `index` 和 `relevance_score`。协议来源是本地 AI SDK 基线 `6c6c221` 的 `packages/voyage/src/reranking/voyage-reranking-model.ts` 及其选项 schema。

[决策] `voyage` 下的供应商选项接受驼峰命名的 `returnDocuments` 和 `truncation`，均为可选布尔值。自定义供应商名称下的选项覆盖标准选项。未知或无效选项在 HTTP 前失败。适配器不通过固定列表限制模型 ID。

[决策] 文本文档原样发送。JSON 对象序列化为字符串，并产生一条兼容性警告。直接模型调用拒绝空文档列表和零值 `top_n`；核心层的空列表快捷返回仍可使用。适配器保留排名顺序，拒绝重复或越界索引、非有限分数、升序分数以及超出请求上限的结果。原始响应体保留返回的文档和用量，不增加规范字段。

[决策] 响应元数据保留请求响应头、原始响应体和模型 ID（优先使用响应中的模型，否则使用请求模型）。HTTP 错误读取 Voyage 的 `detail` 消息，并使用共享的按状态码重试分类。取消与传输行为遵循共享 HTTP 实现。

## 验证范围

[待验证] (PV-031) 本地响应夹具和请求快照验证转换及校验，包括核心 `rerank` 路径。夹具是手写协议示例，并非从 Voyage 服务录制的响应。真实凭据服务验证与录制仍待完成，参见[待验证项](../05-appendix/02-pending-verification.md)。

## 实现记录（2026-09-17）

[事实] 实现在 `crates/providers/ferrin-voyage/src/`；`tests/suite/` 包含十项通过的本地测试，使用 wiremock 和注入的录制传输。覆盖选项、认证请求头、脱敏、异常排名、错误、取消、请求快照以及核心 JSON 文档重排路径。2026-09-17 运行的包级 Clippy（警告视为错误）、两个 rustdoc 示例及文档生成（警告视为错误）均通过。这些运行验证本地适配器，不验证外部服务。

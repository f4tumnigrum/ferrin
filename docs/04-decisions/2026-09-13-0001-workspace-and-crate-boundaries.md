# 0001: 工作区与 crate 边界

- 状态：accepted
- 日期：2026-09-13
- 相关：[Crate 划分与职责](../01-architecture/02-crates.md)

## 背景

【事实】主流供应商 API（OpenAI、Anthropic、Google）在请求形状、流事件与错误体上各不相同，而应用侧需要以统一接口调用；MCP 工具在调用时只需要工具定义与执行接口，不需要生成循环的类型。

【事实】Cargo 工作区允许多个 crate 共享锁文件与 lint 配置并独立发布；单一大 crate 会让所有下游为用不到的供应商与模态付出编译时间，也容易让功能无序堆积到核心。

## 决策

按六层划分 crate：`ferrin-spec`（L0）；`ferrin-schema`、`ferrin-message`（L1）；`ferrin-provider-util`、`ferrin-tool`（L2）；供应商 crate 与 `ferrin-mcp`（L3）；`ferrin-core`（L4）；`ferrin`、`ferrin-otel`、`ferrin-testing`、`ferrin-macros`（L5）。依赖只能自上而下；供应商 crate 与 `ferrin-mcp` 不依赖 `ferrin-core`。

## 依据

- 供应商适配器与 MCP 客户端的编译依赖面最小化，第三方可以独立发布适配器而不引入核心循环。
- 应用侧消息模型与工具定义分离于规范层，使规范层保持纯数据、可序列化。
- 门面 crate 让应用只声明一个依赖并用 feature 选择供应商。

## 备选方案

- 单一 crate 加 feature：编译时间与依赖面随功能膨胀；feature 组合难以全部测试。
- 按模态拆分核心（`ferrin-embed`、`ferrin-image`）：横切基础设施（重试、遥测、Prompt 转换）会被重复或再拆一层；收益不足。

## 影响

- 需要 `xtask publish-order` 维护发布顺序。
- 跨 crate 的类型需要在规范层或 L1 定义，增加前期设计成本。

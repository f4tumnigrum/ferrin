# 架构决策记录流程

[English](../../03-engineering/07-adr-process.md) | **简体中文**

## 1. 目的

架构决策记录（ADR）保存对项目结构、公共契约或工程流程有长期影响的决策及其依据，使后续维护者能理解“为什么”而不仅是“是什么”。

【决策】ADR 文件名以日期前缀，状态为 `proposed`、`accepted`、`rejected`、`deprecated`、`superseded`，由 `docs/04-decisions/README.md` 列出索引。依据：日期前缀给出自然的时间序；状态字段让被取代的决策留档而不误导读者。

## 2. 何时需要 ADR

- 新增或移除 crate；调整 crate 间依赖方向。
- `ferrin-spec` 公共类型或 trait 的破坏性变更。
- 公共 API 形态的全局性规则（构建器约定、错误模型、序列化格式）。
- 引入新的核心外部依赖（HTTP 客户端、Schema 库、运行时）。
- 安全相关机制（签名、URL 策略、密钥处理）的设计或变更。
- 工程流程的重大调整（CI 门禁、发布方式、MSRV 策略）。

局部实现选择（算法、数据结构、模块内部组织）不需要 ADR，在代码注释与 PR 描述中说明即可。

## 3. 文件约定

- 位置：英文主版本 `docs/04-decisions/`，对应中文译本 `docs/zh-CN/04-decisions/`。
- 文件名：`YYYY-MM-DD-NNNN-<kebab-case-title>.md`，`NNNN` 为四位递增编号。
- 语言：英文主版本正文、代码均为英文；本目录为独立中文译本，两版同步维护（[ADR 0018](../04-decisions/2026-09-14-0018-english-primary-documentation.md)）。
- 状态：`proposed` → `accepted` | `rejected`；`accepted` 可转为 `deprecated` 或 `superseded by NNNN`。

## 4. 模板

```markdown
# NNNN: <标题>

- 状态：proposed | accepted | rejected | deprecated | superseded by NNNN
- 日期：YYYY-MM-DD
- 相关：<关联 ADR、issue、PR>

## 背景

<问题、约束、相关事实（标注来源）>

## 决策

<做出的决定，使用肯定句>

## 依据

<技术理由；与备选方案的比较>

## 备选方案

- <方案 A>：<为何未采用>
- <方案 B>：<为何未采用>

## 影响

<对代码、API、依赖、流程的影响；需要跟进的事项；待验证项>
```

## 5. 流程

1. 作者以 `proposed` 状态提交 ADR PR（可与实现 PR 分离）。
2. 至少两名维护者评审；涉及 `ferrin-spec` 的 ADR 需全部活跃维护者知悉。
3. 达成一致后状态改为 `accepted` 合并；否决改为 `rejected` 并保留文件。
4. 实现 PR 引用 ADR 编号。
5. 决策被替代时，新 ADR 引用旧编号，旧 ADR 状态改为 `superseded by`。
6. 两版 ADR 索引随 ADR 同 PR 更新。

## 6. 与本文档集的关系

本文档集中标注为【决策】的条目在初始化阶段已汇总为编号 0001–0012 的 ADR（见 [ADR 索引](../04-decisions/README.md)）。后续设计文档修改若改变已有决策，必须先通过 ADR 流程。

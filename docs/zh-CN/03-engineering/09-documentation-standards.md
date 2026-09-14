# 文档规范

[English](../../03-engineering/09-documentation-standards.md) | **简体中文**

## 1. 文档类型

| 类型 | 位置 | 语言 | 受众 |
| --- | --- | --- | --- |
| 设计文档（本文档集） | `docs/zh-CN/00-05` | 中文正文，英文代码 | 维护者 |
| ADR | `docs/zh-CN/04-decisions/` | 中文正文，英文代码 | 维护者 |
| 供应商文档 | `docs/zh-CN/providers/<name>.md` | 中文正文，英文代码 | 维护者与使用者 |
| API 文档 | rustdoc（源码注释） | 英文 | 使用者 |
| 项目 README | `README.zh-CN.md` | 中文正文，英文代码 | 使用者（仓库首页：特性、快速开始、用法、状态） |
| 设计文档索引 | `docs/zh-CN/README.md` | 中文 | 维护者（目录与文档约定） |
| crate README | `crates/*/README.md` | 英文 | 使用者（crates.io 展示） |
| 示例 | `examples/` | 英文代码与注释 | 使用者 |
| 变更日志 | `CHANGELOG.md` | 英文 | 使用者 |

【决策】英文为主版本，位于 `README.md` 与 `docs/`；独立中文版位于 `README.zh-CN.md` 与 `docs/zh-CN/`。内容冲突时以英文为准，各版内部链接指向同语言章节，通过明确入口切换语言；源码、资源、fixture 与 `docs/api/*.json` 共用。此规则取代仅使用中文设计文档的旧规则，依据见 [ADR 0018](../04-decisions/2026-09-14-0018-english-primary-documentation.md)。

## 2. 写作规则

- 每条陈述标注来源类别：【事实】（附源码路径）、【决策】（附依据）、【待验证】（汇总至附录）。
- 不写入对话过程、需求评价或计划性口号。
- 描述能力时区分“设计目标”与“已实现并验证”；实现完成前不使用“支持”“已实现”等表述描述 Ferrin 自身。
- 一段只说一件事；表格用于并列对照；代码块用于签名与示例。
- 标识符、文件路径、命令一律使用代码格式。
- 引用外部资料时给出可定位的来源：官方文档或规范给出 URL 与章节，依赖 crate 给出版本与模块路径，仓库内代码给出相对路径与函数名。

## 3. 同步义务

| 变更 | 需同步的文档 |
| --- | --- |
| 公共 API 新增/变更 | rustdoc、`docs/02-api/02-api-reference.md`、`CHANGELOG.md` |
| crate 新增/删除 | `docs/01-architecture/02-crates.md`、`docs/03-engineering/02-workspace-layout.md`、发布顺序 |
| 依赖版本变化 | `docs/03-engineering/01-toolchain-and-dependencies.md` 核实记录 |
| 决策变更 | 新 ADR + 对应设计文档段落 |
| 待验证事项关闭 | 移除原标注，结果写入正文并改为【事实】或【决策】；更新附录 |
| 供应商能力变化 | `docs/zh-CN/providers/<name>.md` 能力矩阵 |

【决策】同一次变更同步两版对应页面，保留章节结构、ADR 编号、PV 编号、来源、日期和验证范围。英文使用 `[Fact]`、`[Decision]`、`[Pending verification]`；中文保留原有标记。`docs-lint` 检查两版链接、待验证登记与编号状态一致性；`just doc` 单独检查 rustdoc 警告。依据见 ADR 0018。

## 4. rustdoc 规则

- crate 级文档（`lib.rs` 的 `//!`）包含：一句话定位、在分层中的位置、最小示例、feature 列表（使用 `document-features` 0.2.12 从 `Cargo.toml` 生成）。
- 公共项文档结构：概述 → 细节 → `# Errors` → `# Panics` → `# Examples`。
- 示例必须可编译；需要网络的示例使用 `MockLanguageModel` 或标注 `no_run`。
- 使用 `#[doc(cfg(feature = "..."))]`（`docsrs` cfg 下）标注 feature 门控项。
- 不在 rustdoc 中重复设计文档的背景讨论；链接到仓库文档。

## 5. 供应商文档模板

```markdown
# <Provider>

## 能力矩阵
| 能力 | 支持 | 说明 |
| 语言模型（生成/流式） | ✓ | ... |
| 工具调用 / 供应商工具 | ✓ | web_search, ... |
| 结构化输出 | ✓ | strict mode 限制 ... |
| 推理 | ✓ | effort 映射表 |
| 嵌入 / 图像 / 语音 / 转写 / 重排 / 视频 | ... |
| 文件 / 技能 / 批处理 / 实时 | ... |

## 设置与环境变量
## 供应商选项（provider_options["<key>"]）
## 供应商元数据（provider_metadata["<key>"]）
## 已知限制与警告
## Fixture 清单
```

## 6. 图示

- 架构图使用 ASCII 或 Mermaid；Mermaid 仅在 GitHub 渲染可用时使用，且同时给出文字说明。
- 序列图用于跨组件交互（审批往返、流式管线）。

## 7. 术语

- 术语以[术语表](../00-overview/02-glossary.md)为准；新增术语先更新术语表。
- 中英对照：首次出现时给出英文标识符，之后可只用中文或标识符之一。

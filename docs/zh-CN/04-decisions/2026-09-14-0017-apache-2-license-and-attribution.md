# 0017: 许可改为 Apache-2.0 单许可并署名派生代码

[English](../../04-decisions/2026-09-14-0017-apache-2-license-and-attribution.md) | **简体中文**

- 状态：accepted
- 日期：2026-09-14
- 相关：[工作区布局](../03-engineering/02-workspace-layout.md)第 1、2 节；根目录 `LICENSE`、`NOTICE`；`README.md`「致谢」

## 背景

工作区原以 `MIT OR Apache-2.0` 双许可发布，仓库根目录有 `LICENSE-MIT` 与 `LICENSE-APACHE` 两个文件。

【事实】Ferrin 的设计参考了 Vercel AI SDK 的公开源码与文档；核心生成循环、供应商适配器与部分算法（部分 JSON 修复、消息裁剪等）由其 TypeScript 实现移植到 Rust 并有修改。该项目以 Apache License 2.0 发布，版权归 Vercel, Inc.，仓库不含 NOTICE 文件（2026-09-14 核对本地检出的 `LICENSE` 与目录）。

【事实】Apache License 2.0 第 4 条对派生作品再分发的要求：附带许可证副本；改动过的文件带有显著的改动说明；保留源码中的版权、专利、商标与署名声明；若原作品含 NOTICE 文件，则随派生作品分发其内容。

## 决策

1. 工作区 `license` 字段改为 `Apache-2.0`；删除 `LICENSE-MIT`，`LICENSE-APACHE` 改名为 `LICENSE`。
2. 根目录新增 `NOTICE`：Ferrin 的版权行、派生自 Vercel AI SDK 的说明（含涉及的 crate 列表）、对 OpenAI Codex 工程实践的致谢，以及无关联声明。
3. `LICENSE` 与 `NOTICE` 复制到每个发布的 crate 目录，随 `cargo package` 分发。
4. 含派生代码的 crate 在 crate 级 rustdoc 中加「Attribution」段落；移植最接近的模块在模块级 rustdoc 中加派生说明。新增的派生代码遵循同一规则。
5. `README.md` 增加「致谢」一节；`CONTRIBUTING.md` 的贡献许可改为 Apache-2.0；项目范围文档第 1 节改为「独立实现，设计参考并移植了上游实现」。

## 依据

- 派生自 Apache-2.0 代码的部分不能单独按 MIT 条款提供：MIT 没有 Apache 第 3 条的专利授权，也没有第 4 条关于改动说明与 NOTICE 的要求，双许可会让下游误以为这些部分可以只按 MIT 使用。单一 Apache-2.0 许可与上游一致，义务清晰。
- Apache-2.0 在 Rust 生态与本项目依赖图中被广泛接受，`deny.toml` 的允许列表已包含；对下游的约束与双许可中的 Apache 选项相同。
- `cargo package` 只打包 crate 目录内的文件，根目录的许可证文件不会进入发布包；每个 crate 目录持有副本才能满足随发布附带许可证与声明的要求。

## 备选方案

- 保留双许可，仅对派生文件声明 Apache-2.0：许可状态按文件分裂，crates.io 的 `license` 字段无法表达；拒绝。
- 独立重写派生部分以维持双许可：工作量大，且难以证明结果与已阅读的上游实现无关；拒绝。
- 仅在 README 致谢，不加 NOTICE 与文件说明：不满足 Apache 第 4 条的形式要求；拒绝。

## 后果

- 下游只能按 Apache-2.0 使用 Ferrin；Apache-2.0 与 GPLv2 不兼容，对本项目的目标用户没有实际影响。
- 每个 crate 目录多两份文件；许可证或 NOTICE 变更时需同步 15 份副本。
- 设计文档继续只引用一手来源；上游项目的名称只出现在 `NOTICE`、README「致谢」、项目范围文档第 1 节、本 ADR 与相关 rustdoc 说明中。

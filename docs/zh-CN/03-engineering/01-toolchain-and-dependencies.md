# 工具链与依赖版本

[English](../../03-engineering/01-toolchain-and-dependencies.md) | **简体中文**

版本核实日期：2026-09-13。核实方式在每节注明。设计时选用的都是当日最新官方稳定版本；实现开始时需重新核实并更新本文档。

## 1. Rust 工具链

| 项 | 版本 | 核实方式 |
| --- | --- | --- |
| Rust stable | 1.98.1 | `rustup check` 报告 stable 可更新至 `1.98.1 (48a229cea 2026-09-01)`；GitHub `rust-lang/rust` Releases 页面显示 1.98.1 发布于 9 月 3 日，1.98.0 发布于 8 月 20 日。 |
| 本机已安装 | `1.98.1-aarch64-apple-darwin`（rustc 1.98.1 (48a229cea 2026-09-01)、cargo 1.98.1 (797e8a9bc 2026-08-05)）；stable 默认工具链仍为 1.98.0 | `rustup toolchain list`、`rustup run 1.98.1 rustc --version`（2026-09-13 复核）。仓库内 `rust-toolchain.toml` 选择 1.98.1，全部质量门禁已在该版本上运行。 |
| rustup | 1.29.1 | `rustup --version` |
| Edition | 2024 | Rust 1.85 起稳定，当前最新 edition。 |

`rust-toolchain.toml`：

```toml
[toolchain]
channel = "1.98.1"
components = ["clippy", "rustfmt", "rust-src", "llvm-tools-preview"]
profile = "minimal"
```

MSRV 策略见[版本与发布](06-versioning-and-release.md)。

## 2. 运行时依赖

版本来自 crates.io API `max_stable_version` 字段（2026-09-13 查询）。列出的是工作区 `[workspace.dependencies]` 的目标版本；`Cargo.toml` 中以 `major.minor` 精度声明并由 `Cargo.lock` 锁定。

| Crate | 版本 | 用途 | 使用 crate |
| --- | --- | --- | --- |
| `tokio` | 1.53.1 | 异步运行时（features: `rt`, `sync`, `time`, `macros`, `net`, `fs`, `process`） | 全部 |
| `tokio-util` | 0.7.19 | `CancellationToken`、编解码工具 | spec, core, provider-util |
| `tokio-stream` | 0.1.19 | 通道到 Stream 适配 | core |
| `futures-core` | 0.3.34 | `Stream` trait（公共 API 中唯一的 futures 依赖） | spec |
| `futures-util` | 0.3.34 | 流组合子 | core, provider-util |
| `pin-project-lite` | 0.2.17 | 手写 Future/Stream 的投影 | core |
| `serde` | 1.0.229 | 序列化（`derive`） | 全部 |
| `serde_json` | 1.0.151 | JSON（`preserve_order`, `raw_value`） | 全部 |
| `schemars` | 1.2.2 | JSON Schema 派生 | schema |
| `jsonschema` | 0.56.0 | 动态 JSON Schema 校验（可选 feature） | schema |
| `thiserror` | 2.0.20 | 错误派生 | 全部 |
| `bytes` | 1.12.1 | 二进制数据 | 全部 |
| `http` | 1.5.0 | `HeaderMap`、`StatusCode`、`Method` | spec, provider-util |
| `url` | 2.5.8 | URL 解析 | spec, provider-util |
| `reqwest` | 0.13.5 | 默认 HTTP 传输（features: `rustls`, `http2`, `stream`；关闭默认 features。【事实】0.13 起 feature 名为 `rustls` 而非 `rustls-tls`，见 PV-015；【决策】2026-09-13 不启用 `multipart`/`json`/`charset`，见 HTTP 文档第 11 节） | provider-util |
| `rustls` | 0.23.45 | TLS（经 reqwest 间接引入，默认加密提供者 aws-lc；【事实】2026-09-14 起不再在工作区表中直接声明，无成员直接使用） | —（传递依赖） |
| `rustls-platform-verifier` | 0.7.0 | 系统证书校验器（【事实】reqwest 0.13 默认启用；feature `platform-verifier` 仅用于直接配置它）。`webpki-roots` 不再需要 | provider-util（feature） |
| `tokio-tungstenite` | 0.30.0 | WebSocket（实时会话） | core（feature `realtime`）, openai |
| `chrono` | 0.4.45 | 时间戳（`serde`, `clock`）；`Retry-After` 日期解析 | spec, provider-util |
| `rand` | 0.10.2 | 前缀随机 ID | provider-util |
| `base64` | 0.23.1 | base64 与 base64url | spec, provider-util, core |
| `data-url` | 0.3.2 | 【决策】不采用（PV-002）：`ferrin-message` 自实现 RFC 2397 解析 | — |
| `hmac` | 0.13.0 | 审批签名 | core |
| `sha2` | 0.11.0 | 审批签名、指纹 | core, tool, mcp |
| `secrecy` | 0.10.3 | 密钥类型 | provider-util, core, providers |
| `zeroize` | 1.9.0 | 【决策】不直接依赖（2026-09-13）：密钥清零由 `secrecy` 传递引入，工作区无直接使用点 | — |
| `ipnet` | 2.12.2 | 私网网段判定 | provider-util |
| `indexmap` | 2.14.2 | 有序工具集 | tool |
| `regex` | 1.13.1 | `smooth_stream` 自定义切分、推理标签提取 | core |
| `unicode-segmentation` | 1.13.3 | 词边界切分 | core |
| `tracing` | 0.1.44 | 日志与 span | 全部 |
| `arc-swap` | 1.9.2 | 【决策】不引入（PV-024）：全局默认注册表用 `OnceLock` 一次性设置（ADR 0008），无热替换需求；中间件与注册表均为不可变值 | — |
| `opentelemetry` | 0.32.0 | OTel API | otel |
| `opentelemetry_sdk` | 0.32.1 | OTel SDK（`ferrin-otel` 仅在测试中使用，feature `testing` 提供内存导出器；库本身只依赖 API） | otel（dev） |
| `opentelemetry-semantic-conventions` | 0.32.1 | 【决策】不引入（PV-014）：其 `GEN_AI_*` 常量已弃用，`ferrin-otel` 自定义常量 | — |
| `tracing-opentelemetry` | 0.33.0 | tracing 与 OTel 桥接 | otel |
| `syn` / `quote` / `proc-macro2` | 3.0.5 / 1.0.47 / 1.0.107 | 过程宏（2026-09-13 crates.io 核实；【事实】`syn` 3 与生态中的 `syn` 2 并存于依赖图） | macros |
| `hyper` / `hyper-util` / `http-body-util` | 1.11.1 / 0.1.20 / 0.1.5 | 流式 fixture 服务器（PV-026） | testing |

【决策】不引入的依赖及原因：`async-trait`（使用 RPITIT + Dyn 适配）、`eventsource-stream`（自实现 SSE）、`infer`（自维护媒体类型签名表）、`anyhow`（库不使用 `anyhow`，仅示例与 xtask 可用）、`once_cell`（标准库 `OnceLock`/`LazyLock` 已稳定）、`lazy_static`（同上）。

## 3. 开发与测试依赖

| Crate | 版本 | 用途 |
| --- | --- | --- |
| `wiremock` | 0.6.5 | HTTP 模拟服务器，fixture 回放 |
| `insta` | 1.48.0 | 快照测试（请求体、事件序列） |
| `pretty_assertions` | 1.4.1 | 差异输出 |
| `proptest` | 1.11.0 | 属性测试（部分 JSON 修复、SSE 解码） |
| `criterion` | 0.8.2 | 基准测试（features `async_tokio`、`html_reports`；见[测试规范](04-testing.md)第 11 节） |
| `trybuild` | 1.0.121 | 过程宏编译失败用例 |
| `tracing-subscriber` | 0.3.23 | 测试中的日志捕获 |
| `static_assertions` | 1.1.0 | `Error` 尺寸断言（PV-013） |
| `anyhow` / `clap` / `cargo_metadata` | 1.0.104 / 4.6.6 / 0.23.1 | 仅 `xtask` 与示例 |

## 4. 命令行工具

| 工具 | 版本 | 用途 |
| --- | --- | --- |
| `cargo-nextest` | 0.9.144 | 测试执行器（`just test`） |
| `cargo-insta` | 1.48.0 | 快照审阅 |
| `cargo-deny` | 0.20.2 | 许可证、漏洞公告、重复依赖检查 |
| `cargo-shear` | 1.13.4 | 未使用依赖检查（CI 以 `cargo shear --deny-warnings` 运行） |
| `cargo-semver-checks` | 0.50.0 | 发布前 API 兼容性检查 |
| `cargo-llvm-cov` | 0.9.1 | 覆盖率 |
| `cargo-hack` | 0.6.45 | feature 组合与 MSRV 矩阵检查 |
| `typos-cli` | 1.50.1 | 拼写检查（Rust 生态工具，无 Python 运行时依赖） |
| `release-plz` | 0.3.165 | 【决策】不采用（PV-025，见[版本与发布](06-versioning-and-release.md)第 5 节） |
| `git-cliff` | 2.14.1 | 变更日志生成（`cliff.toml`，`just changelog <crate>`） |
| `just` | 1.58.0 | 任务入口（`justfile`） |
| `cargo-binstall` | 1.23.0 | 安装上述工具的预编译二进制 |

## 5. 版本核实记录格式

每次核实在本文档追加一行：

| 日期 | 范围 | 结果 | 执行人 |
| --- | --- | --- | --- |
| 2026-09-13 | Rust 工具链、全部运行时与开发依赖 | 首次建立 | 设计阶段 |
| 2026-09-13 | 1.98.1 工具链、§4 全部命令行工具安装、骨架 `Cargo.lock` 解析（396 个包）、`syn`/`quote`/`proc-macro2`/`hyper` 系列/`static_assertions`/`anyhow`/`clap`/`cargo_metadata` 版本 | 全部与本文一致；`cargo deny check` 四项通过 | 准备阶段 |
| 2026-09-14 | `opentelemetry`、`opentelemetry_sdk`、`tracing-opentelemetry`（crates.io API `max_stable_version`） | 0.32.0 / 0.32.1 / 0.33.0，与本文及 `Cargo.lock` 一致 | 实现 `ferrin-otel` |
| 2026-09-14 | 工作区全部 52 个直接外部依赖（`cargo xtask check-versions`，crates.io 稀疏索引，取未 yank 的最高非预发布版本） | `Cargo.lock` 解析版本全部为最新稳定版 | 实现 `check-versions` 后首次运行 |
| 2026-09-14 | `cargo shear --deny-warnings` 清理：从工作区表移除无成员使用的 `assert_matches`、`async-stream`、`criterion`、`data-url`（PV-002 已决定不采用）、`rustls`（仅经 reqwest 传递）、`serde_with`、`subtle`（审批签名用 `hmac::Mac::verify_slice` 常量时间比较）、`tempfile`、`tokio-test`、`uuid`（ID 由 `ferrin_provider_util::IdGenerator` 生成）；从成员清单移除未使用的 `ferrin-core`（`subtle`、`insta`、`proptest`、`tokio-test`、`assert_matches`、`tracing-subscriber`、`criterion`）、`ferrin-provider-util`（`percent-encoding`、`tracing`、`proptest`、`tokio-test`）、`ferrin-testing`（`futures-core`、`thiserror`、`tracing`）、`ferrin-openai-compatible`（`futures-util`、`regex`）声明；`ferrin-message` 的 `serde_json` 改为开发依赖 | `cargo shear --deny-warnings` 无报告 | 清理阶段 |
| 2026-09-14 | `criterion` 0.8.2（crates.io 最新稳定版，2026-02-04 发布，Apache-2.0 OR MIT，`rust-version` 1.86）重新加入工作区，作为 9 个 crate 的开发依赖用于基准测试 | `cargo shear --deny-warnings` 无报告，`cargo deny check` 四项通过，`cargo hack check --each-feature` 通过；`bench.yml` 只引用已固定 SHA 的 `actions/checkout`、`dtolnay/rust-toolchain`、`Swatinem/rust-cache`、`actions/upload-artifact` | 基准测试阶段 |
| 2026-09-15 | `rustls`，crates.io 官方 API `max_stable_version` 与未 yank 的发布记录 | 【事实】0.23.45 为最新稳定版，修复 [RUSTSEC-2026-0285](https://rustsec.org/advisories/RUSTSEC-2026-0285)；将传递依赖锁定版本从 0.23.44 升级，直接依赖约束不变 | 安全审查 I04 |

核实脚本 `cargo xtask check-versions` 读取 `Cargo.toml` 中的版本并与 crates.io 比较，输出过期项；CI 每周执行一次并开 issue（见 [CI 与质量门禁](05-ci-and-quality-gates.md)）。

## 6. 已核实的 API 事实

以下事项由 `verification/` 中的原型编译与运行确认（2026-09-13）：

- 【事实】（PV-015）reqwest 0.13 相对 0.12 的破坏性变更：默认 TLS 后端 rustls（feature `rustls-tls` → `rustls`）、默认加密提供者 aws-lc、默认校验器 `rustls-platform-verifier`、`query`/`form` 改为可选 feature、`trust-dns` 等弃用项移除、TLS 构建器方法改名（旧名软弃用）。`resolve_to_addrs`、`redirect::Policy::none()` 保留；0.13.5 新增 `Error::is_dns()` 与 `http1_max_headers`。
- 【事实】（PV-022）`hmac` 0.13 / `sha2` 0.11 / `subtle` 2.6 / `secrecy` 0.10 可共同编译；`new_from_slice` 来自 `hmac::KeyInit`。
- 【事实】（PV-023）`base64` 0.23 保持 `Engine` API（`base64::prelude::BASE64_STANDARD`、`BASE64_URL_SAFE_NO_PAD`）；`rand` 0.10 的入口为 `rand::rng()`，随机字符串通过 `rand::RngExt::sample_iter(rand::distr::Alphanumeric)`（`Rng` trait 更名为 `RngExt`）；`tokio-tungstenite` 0.30 的 TLS feature 为 `rustls-tls-webpki-roots` / `rustls-tls-native-roots`（默认 features `connect`、`handshake`），类型 `Connector`、`MaybeTlsStream` 可用。
- 【事实】（PV-014）`opentelemetry` 0.32.0 + `opentelemetry_sdk` 0.32.1 + `tracing-opentelemetry` 0.33.0 兼容。
- 【事实】（PV-004）`schemars` 1.2.2 的 `SchemaSettings::draft07()` 输出符合预期形状；`jsonschema` 0.56 的 draft-07 校验在实现阶段随 `ferrin-schema` 的动态校验测试覆盖（尚未单独原型）。
- 【事实】`cargo deny` 报告的重复主版本：`base64` 0.22（hyper-util、reqwest、wiremock）与 0.23、`rand`/`rand_core` 0.9（opentelemetry_sdk）与 0.10、`syn` 2（过程宏生态）与 3、`getrandom` 0.3 与 0.4。【决策】保持设计时最新主版本，`multiple-versions = "warn"`；当 reqwest、opentelemetry 升级到相同主版本后重复自然消失，不为此降级。
- 【事实】本机 `~/.cargo/config.toml` 把 crates.io 替换为 `sparse+https://rsproxy.cn/index/` 镜像；`Cargo.lock` 解析到的版本与 crates.io API 查询结果一致。
- 【事实】（2026-09-14，推送前复核）工作流中固定 SHA 的 GitHub Actions 与各自最新发布一致：`actions/checkout` v7.0.1、`dtolnay/rust-toolchain` v1、`Swatinem/rust-cache` v2.9.2、`taiki-e/install-action` v2.87.12、`EmbarkStudios/cargo-deny-action` v2.1.1、`codecov/codecov-action` v7.0.0、`crate-ci/typos` v1.50.1、`peter-evans/create-issue-from-file` v6.0.0、`softprops/action-gh-release` v3.0.3；新引用 `obi1kenobi/cargo-semver-checks-action` v2.9（`6b69fcf4…`）已固定。`cargo update --dry-run` 显示 4 个间接依赖有新补丁（`cc` 1.4.6、`fancy-regex` 0.19.2、`lru-slab` 0.1.3、`tinyvec` 1.13.3），交由 Dependabot 每周更新；直接依赖仍全部为最新（`cargo xtask check-versions`）。
- 【事实】（2026-09-14，移除 Codecov 上传后）`coverage.yml` 改用 `actions/upload-artifact` v7.0.1（`043fb46d…`，GitHub Releases 最新版本，当日经 API 查询）保留 lcov 报告；`codecov/codecov-action` 不再被任何工作流引用。

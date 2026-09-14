# CI 与质量门禁

[English](../../03-engineering/05-ci-and-quality-gates.md) | **简体中文**

## 1. 工作流

【决策】工作流按关注点拆分为独立作业（格式、Clippy、按平台矩阵运行的 nextest、文档、依赖审计等），工具链版本通过 `dtolnay/rust-toolchain` 固定，全部 action 以 commit SHA 锁定。依据：独立作业让失败原因一目了然并可并行执行；SHA 锁定防止 action 标签被篡改或漂移。

Ferrin 工作流：

| 工作流 | 触发 | 作业 |
| --- | --- | --- |
| `ci.yml` | PR、push 到 main | `fmt`、`clippy`、`test`（3 平台矩阵，nextest + doctest）、`doc`（含 `api-snapshot --check`）、`deny`、`shear`、`examples`、`msrv`、`features`、`docs-lint`、`package`、`lockfile` |
| `semver.yml` | PR 改动 `Cargo.toml` 或 `crates/**`（含加标签事件） | 以基线分支可达的最近 `v*` tag 为基线运行 `cargo semver-checks`；尚无发布 tag 时输出提示并跳过 |
| `coverage.yml` | push 到 main、PR | `cargo llvm-cov`；摘要写入作业摘要，lcov 报告作为构建产物保留 14 天 |
| `live-tests.yml` | 手动触发、每周定时 | 在线测试（需要 secret） |
| `bench.yml` | 手动触发（输入 `filter`） | `cargo bench --workspace --all-features`（criterion），`target/criterion` 报告作为构建产物保留 30 天；不是门禁，基准代码由 `ci.yml` 的 `clippy` 作业（`--all-targets`）编译检查（见[测试规范](04-testing.md)第 11 节） |
| `versions.yml` | 每周定时、手动触发 | `cargo xtask check-versions`，过期依赖开 issue；`cargo deny check advisories`，有漏洞开 issue |
| `release.yml` | tag `v*` | 按依赖顺序 `cargo publish`（见[版本与发布](06-versioning-and-release.md)） |
| `typos.yml` | PR | `typos` |

## 2. 门禁清单

PR 合并的必要条件（GitHub 分支保护）：

【事实】（2026-09-14）仓库公开前使用 GitHub Free 方案，分支保护与规则集不可用，以下门禁由约定执行。仓库公开后在 `main` 上启用了规则集 `protect-main`（禁止删除与强推，见第 8 节）；必需状态检查与审阅者要求尚未启用，因为维护者当前直接向 `main` 推送，改为 PR 工作流后再启用（`ci.yml` 全部作业为必需检查）。

1. `cargo fmt --all -- --config imports_granularity=Item --check` 通过。
2. `cargo clippy --workspace --all-targets --all-features -- -D warnings` 通过（Linux）。
3. `cargo nextest run --workspace --all-features --no-fail-fast` 与 `cargo test --workspace --all-features --doc`（nextest 不运行 doctest）在 Linux、macOS、Windows 通过。
4. `RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo doc --workspace --no-deps --all-features` 通过（`missing_docs` 在 CI 通过 `RUSTFLAGS=-D missing_docs` 对发布 crate 提升为错误）。
5. `cargo deny check`（advisories、licenses、bans、sources）通过。
6. `cargo shear --deny-warnings` 通过。
7. `cargo hack check --workspace --each-feature --no-dev-deps` 通过（feature 独立可编译）。
8. `cargo +1.98.0 check --workspace --all-features --locked`（MSRV 作业；`rust-version = "1.98"` 对应 1.98.0，工具链固定 1.98.1）。
8a. `python3 scripts/docs_lint.py`：文档相对链接可解析；附录以外每条【待验证】必须带 `PV-xxx` 编号并在附录中登记。
9. 全部示例编译。
10. 工作树干净（各作业结束时 `git diff --exit-code`，无未提交的生成文件，如快照、`docs/api`）；`Cargo.lock` 与清单一致（`lockfile` 作业：`cargo update --workspace --locked`，锁文件需要变更时失败）。
10a. `package` 作业：`cargo package --locked -p <crate>…`（按 `cargo xtask publish-order` 的顺序列出全部可发布 crate），从打包后的源码构建每个 crate，与 `release.yml` 发布的内容一致。
11. 至少一名维护者批准；涉及 `ferrin-spec` 公共类型变更需两名批准并关联 ADR。

【事实】骨架阶段 `cargo shear` 曾报告 292 项未使用依赖（crate 先按设计声明依赖、尚无代码使用），`ci.yml` 的 `shear` 作业当时以 `continue-on-error: true` 运行。2026-09-14 首次完整构建后清理了剩余的未使用声明（见[工具链与依赖](01-toolchain-and-dependencies.md)第 5 节当日记录），`cargo shear --deny-warnings` 无报告，`continue-on-error` 已移除，`shear` 作业成为阻断门禁。

【事实】（2026-09-14，首次推送完整构建前的复核）本地按 CI 命令逐项复跑发现两处问题并已修正：`cargo test --doc` 暴露 `ferrin-mcp` 的 `McpClient` 文档示例不能编译（`?` 无法把 `url::ParseError` 转为 `McpError`；此前只跑 nextest，doctest 未被覆盖，`just doctest` 与 `just check-all` 已补上）；原 `clean-worktree` 作业用 `cargo generate-lockfile` 重新解析全部依赖到最新版本再比较 `Cargo.lock`，在索引有新补丁版本时必然失败（当日 `cargo update --dry-run` 显示 4 个间接依赖有新补丁），已改为 `lockfile` 作业的 `cargo update --workspace --locked`。`x86_64-pc-windows-msvc` 的交叉 `cargo check` 在 macOS 上因 `aws-lc-sys` 需要 Windows SDK 头文件而无法完成，Windows 构建只由 CI 矩阵验证。

【事实】（2026-09-14，首次完整构建推送后的 CI 运行）run 34797869442：14 个作业中 12 个通过；`doc` 失败于 `api-snapshot --check`（`ferrin-spec.json` 的 trait 对象自动 trait 顺序随目标不同，见[工作区布局](02-workspace-layout.md)第 6 节），`test (windows-2025)` 失败于 `tool_macro_ui` 超时（trybuild 冷构建超过 360 s，其余 653 个测试通过）。run 34798946529（跳过 Windows 上的 trybuild 用例、打印摘要差异后）：除 `doc` 外全部通过，Windows 作业 653 个测试通过并记录了 PV-028 的解析器输出。修正渲染顺序后的第三次运行 run 34799653563：14 个作业全部通过（首次全绿）。`coverage.yml` 两次均成功（未配置 `CODECOV_TOKEN`，上传步骤 `fail_ci_if_error: false`）。

## 3. `cargo deny` 配置要点

```toml
[graph]
all-features = true

[advisories]
version = 2
yanked = "deny"

[licenses]
version = 2
allow = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Unicode-3.0", "Zlib", "MPL-2.0", "MIT-0", "CDLA-Permissive-2.0"]

[bans]
multiple-versions = "warn"
wildcards = "deny"
allow-wildcard-paths = true
deny = [
    { crate = "openssl", reason = "rustls only" },
    { crate = "openssl-sys", reason = "rustls only" },
    { crate = "native-tls", reason = "rustls only" },
    { crate = "async-trait", reason = "use RPITIT + Dyn adapters" },
]

[sources]
unknown-registry = "deny"
unknown-git = "deny"
```

【事实】骨架依赖图中 `MIT-0` 来自 `borrow-or-share`（`jsonschema` → `referencing` → `fluent-uri`），`CDLA-Permissive-2.0` 来自 `webpki-root-certs`（`rustls-platform-verifier` 携带的 Mozilla 根证书数据），两者均为宽松许可，加入允许列表。【决策】`ferrin-provider-util` 不提供 `native-tls` feature：`[graph] all-features = true` 下该 feature 会把 `openssl` 引入依赖图并触发禁用项；系统信任库的需求由 reqwest 0.13 默认的 `rustls-platform-verifier` 满足。

【事实】（2026-09-14）`[workspace.dependencies]` 的 `ferrin-testing` 改为不带版本的 path 依赖后（见[版本与发布](06-versioning-and-release.md)第 3 步的发布记录），`ci.yml` 的 `deny` 作业在 run 34814996884（9ac9c5c）与 run 34815425396（00e9b61）失败：`cargo deny` 0.20.2 对每个把 `ferrin-testing` 列为开发依赖的 crate 报 `error[wildcard]: found 2 wildcard dependencies`，其余作业不受影响。cargo-deny 文档（`docs/src/checks/bans/cfg.md`，当日读取）说明 `allow-wildcard-paths`：开启后，path/git 型 `dev-dependencies` 在公开与私有 crate 中都不再告警或报错，公开 crate 的 path/git 型 `dependencies`/`build-dependencies` 仍然报错。

【决策】`[bans]` 设置 `allow-wildcard-paths = true`，`wildcards` 保持 `"deny"`。依据：Cargo 打包时剔除不带版本要求的路径型开发依赖，发布出的清单不含通配版本，这一情形不是该规则要防止的问题；普通依赖的通配版本仍被拒绝。修正后本地 `cargo deny check` 四项通过。

## 4. 平台矩阵

| 平台 | 运行器 | 说明 |
| --- | --- | --- |
| Linux x86_64 | `ubuntu-24.04` | 全量作业 |
| macOS arm64 | `macos-15` | test |
| Windows x86_64 | `windows-2025` | test（stdio 传输、路径处理） |

## 5. 缓存与时长

- 使用 `Swatinem/rust-cache`（以 SHA 锁定）缓存 `target/` 与 registry。
- `ci-test` profile 降低测试二进制体积。
- 目标：PR 全量 CI 在 15 分钟内完成；超过时优先拆分测试二进制或减少 `--all-features` 组合。

## 6. 安全相关检查

- `cargo deny check`（含 advisories）在每个 PR 与 push 上运行；`versions.yml` 每周另跑一次 `cargo deny check advisories`，发现漏洞开 issue（与过期依赖 issue 同一工作流）。
- 【决策】（PV-027）使用 Dependabot（`.github/dependabot.yml`）：cargo 生态每周一、次版本与补丁分组为单个 PR、`verification/` 每月、GitHub Actions 每周分组。依据：GitHub 原生、无需额外应用授权、支持 `Cargo.lock` 更新与分组；Renovate 的额外能力（自动合并、跨仓库预设）当前无需求。
- GitHub Actions 以 commit SHA 固定版本，由 Dependabot 更新。
- CI secret 只由 `live-tests.yml`（供应商密钥）与 `release.yml`（`CARGO_REGISTRY_TOKEN`）读取；`pull_request` 触发的作业不需要 secret。

## 7. 生成文件一致性

以下文件由命令生成并提交，CI 重新生成后比较差异：

- `docs/api/*.json`：`cargo xtask api-snapshot`（rustdoc JSON 公共 API 摘要，用于评审 API 变化）。【事实】（2026-09-14）已实现，`doc` 作业在 `cargo doc` 之后运行 `cargo xtask api-snapshot --check`，摘要与代码不一致时作业失败；改动公共 API 的提交需先重新生成（实现细节见[工作区布局](02-workspace-layout.md)第 6 节）。
- `crates/providers/*/docs/options-schema.json`：供应商选项 JSON Schema。
- `insta` 快照。

## 8. 仓库设置

【事实】（2026-09-14，仓库公开当日通过 GitHub API 设置并读回确认）

- 安全功能：私密漏洞报告（`SECURITY.md` 指向的表单）、Dependabot 漏洞警报、Dependabot 安全更新、密钥扫描与推送保护均已启用；非供应商模式与有效性检查在本方案下不可用，保持关闭。CodeQL 代码扫描使用默认设置（`default` 查询集，自动检测语言）。
- 规则集 `protect-main`（目标为默认分支，无绕过者）：禁止删除分支、禁止强推。重写历史需先停用该规则集。
- Actions：允许范围为 GitHub 官方、已验证发布者与工作流中固定 SHA 使用的第三方 Action（`EmbarkStudios/cargo-deny-action`、`Swatinem/rust-cache`、`crate-ci/typos`、`dtolnay/rust-toolchain`、`obi1kenobi/cargo-semver-checks-action`、`peter-evans/create-issue-from-file`、`softprops/action-gh-release`、`taiki-e/install-action`）；工作流令牌默认只读；来自 fork 的 PR 工作流对所有外部贡献者都需要审批。新增 Action 时需同步更新允许列表。
- 合并方式仅 squash，提交标题取 PR 标题、正文取 PR 描述（与 Conventional Commits 一致），合并后自动删除分支，建议更新落后的 PR 分支。
- 描述与主题已填写；社交预览图、发布令牌与在线测试密钥需在网页端配置。

【决策】规则集不设绕过者、不含必需状态检查。依据：无绕过者时误操作的强推会被拒绝，而有意的历史重写只需临时停用规则集；必需状态检查会拒绝未经 CI 的直接推送，与当前直接向 `main` 推送的工作方式冲突，采用 PR 工作流后再加入。

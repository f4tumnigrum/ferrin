# 版本与发布

[English](../../03-engineering/06-versioning-and-release.md) | **简体中文**

## 1. 语义化版本

- 全部 crate 遵循 Cargo 的 SemVer 约定：`0.y.z` 阶段 `y` 递增表示可能包含破坏性变更，`z` 递增为兼容变更；`1.0` 后 `major.minor.patch` 标准语义。
- 破坏性变更的判定以 `cargo semver-checks` 结果为准，并辅以人工评审下列不受工具覆盖的情形：`#[non_exhaustive]` 枚举新增变体对下游 `match` 的影响（兼容）、默认行为变化（如默认停止条件、默认超时）视为破坏性、序列化格式变化视为破坏性。
- 公共 API 白名单中的第三方类型（见 [API 设计原则](../02-api/01-api-design-principles.md)第 4 节）主版本升级视为 Ferrin 的破坏性变更。

## 2. 版本联动

| 变更所在 crate | 必须联动 |
| --- | --- |
| `ferrin-spec`（破坏性） | 所有供应商 crate、`ferrin-tool`、`ferrin-message`、`ferrin-core`、`ferrin-mcp`、`ferrin-testing`、`ferrin` 同步升级次版本 |
| `ferrin-spec`（兼容） | 无 |
| `ferrin-core`（任意） | `ferrin` 门面版本跟随 |
| 供应商 crate | 仅自身；`ferrin` 门面更新依赖版本 |

【决策】`0.y` 阶段所有 crate 共享同一版本号（`workspace.package.version`），每次发布整体递增。依据：避免早期阶段维护 crate 间复杂的兼容矩阵。进入 `1.0` 后改为独立版本，届时更新本文档。

## 3. MSRV 策略

- `rust-version` 与 `rust-toolchain.toml` 保持一致（当前 1.98）。
- MSRV 提升只在次版本（`0.y`）或主版本发布中进行，变更日志注明。
- 不承诺支持低于当前稳定版两个次版本以上的工具链。

## 4. 弃用

- 弃用项使用 `#[deprecated(since = "0.y.0", note = "use X instead")]`，至少保留一个次版本后移除。
- 不为弃用项提供别名导出链（如带 `experimental_` 前缀的旧名）；弃用说明直接指向替代 API。

## 5. 变更日志

- 每个 crate 维护 `CHANGELOG.md`，格式遵循 Keep a Changelog（`Added`、`Changed`、`Deprecated`、`Removed`、`Fixed`、`Security`）。
- 条目在 PR 中随代码提交；发布 PR 把每个 crate 的 `Unreleased` 段改为 `## [x.y.z] - 日期`，`release.yml` 在发布前校验 tag 与 `workspace.package.version` 一致、且每个可发布 crate 的 `CHANGELOG.md` 含该版本段（纯内部重构可在段内标注 `No user-facing changes`）。
- 【决策】（PV-025）使用 `git-cliff` 2.14.1 生成条目（`cliff.toml`，`just changelog <crate>` 以 `--include-path crates/<crate>/**` 生成该 crate 的 `Unreleased` 段），版本号由发布 PR 统一修改 `workspace.package.version`，发布由 `release.yml` 按 `cargo xtask publish-order` 执行。不采用 `release-plz`：【事实】其 `version_group` 只对“有变更的包”统一版本（配置文档：The version group is considered only when packages contain changes），与第 2 节“`0.y` 阶段全部 crate 同步递增”的规则冲突；其变更日志渲染本身也由 git-cliff 完成，直接使用 git-cliff 更简单。

## 6. 发布流程

1. 发布 PR：更新 `workspace.package.version`、各 `CHANGELOG.md`（`Unreleased` → 版本段）、`docs/` 中的版本引用；`semver.yml`（以上一发布 tag 为基线）与 `ci.yml` 的 `package` 作业通过。
2. 合并后打 tag `v0.y.z`。
3. `release.yml`（环境 `release`，secret `CARGO_REGISTRY_TOKEN`）先校验 tag 与版本、变更日志段，再把 `cargo xtask publish-order` 列出、且 crates.io 上尚无该版本（`cargo info` 查不到）的 crate 以一次 `cargo publish -p <a> -p <b> … --locked` 发布：Cargo 自行按依赖顺序上传，等待每个 crate 在索引可见后再发布其依赖方，并把本次发布集合内的包提供给彼此的验证构建。遇到 crates.io 对新 crate 的发布频率限制（HTTP 429，响应正文给出可重试时间）时，工作流等待到该时间后对仍未发布的 crate 重试，最多 40 轮。失败后重跑时已发布的 crate 被跳过，因此可从中断点继续。
4. 发布 GitHub Release：正文由 `git cliff --latest` 从上一 tag 以来的约定式提交生成，并附根 `CHANGELOG.md`。
5. docs.rs 构建检查：所有 crate `all-features` 文档构建成功。

【决策】（2026-09-14）首个版本用 crates.io API token（`CARGO_REGISTRY_TOKEN`）发布，工作流权限只保留 `contents: write`。依据：crates.io 的 Trusted Publishing 需要 crate 已存在并在 crates.io 上配置 GitHub 仓库为可信发布者，首次发布无法使用；首个版本发布后可改为 `rust-lang/crates-io-auth-action`（需 `id-token: write`），届时更新本节。

【事实】（2026-09-14）本地无法完整预演发布：本机 `~/.cargo/config.toml` 把 crates.io 替换为镜像源，`cargo package`/`cargo publish --dry-run` 对未发布的工作区内依赖（如 `ferrin-provider-util` → `ferrin-spec`）报“no matching package”，因为 Cargo 1.90 起的多包打包只对 crates.io 源叠加本地包；改用不含镜像配置的临时 `CARGO_HOME` 从仓库目录外调用可绕过：当日以此方式对 15 个可发布 crate 执行 `cargo package --locked -p …`（含从打包源码的验证构建）全部成功，约 6 分钟；`cargo publish --dry-run` 未另行执行（同一打包与验证步骤已覆盖，上传步骤只能在发布时验证）。CI 的 `package` 作业在无镜像的运行器上执行同一检查。

发布顺序（依赖拓扑）：`ferrin-spec` → `ferrin-schema` → `ferrin-message` → `ferrin-provider-util` → `ferrin-tool` → `ferrin-macros` → 供应商 crate、`ferrin-mcp` → `ferrin-core` → `ferrin-otel`、`ferrin-testing` → `ferrin`。

【事实】（2026-09-14，v0.1.0 发布记录）逐个 `cargo publish -p <crate>` 在第 6 个 crate `ferrin-openai-compatible` 失败：其开发依赖 `ferrin-testing`（发布顺序忽略开发依赖，该 crate 排在其后）带版本号，Cargo 打包时到 crates.io 索引解析该依赖，报 “no matching package named `ferrin-testing`”；此前 5 个 crate（`ferrin-macros`、`ferrin-spec`、`ferrin-message`、`ferrin-provider-util`、`ferrin-schema`）已成功上传。同一次发布还确认：crates.io 令牌的 Crates 限定填精确名 `ferrin` 时对其他 14 个 crate 返回 403，账号邮箱未验证时返回 400 “A verified email address is required”。

【决策】（2026-09-14）两项修正：`release.yml` 改为对全部未发布 crate 执行一次多包 `cargo publish`（见第 3 步）；`[workspace.dependencies]` 中的 `ferrin-testing` 改为仅 `path`，不带 `version`。依据：多包发布让 Cargo 用本次发布集合满足验证构建的依赖，与 CI `package` 作业和本地多包 `cargo package` 的行为一致；`ferrin-testing` 在工作区内只作为开发依赖使用，Cargo 打包时会剔除不带版本要求的路径型开发依赖，发布出的清单不再引用它，逐个发布也不会再被它阻塞。`ferrin-testing` 自身仍正常发布，供下游在自己的开发依赖中使用。`deny.toml` 相应设置 `allow-wildcard-paths = true`，否则 `cargo deny` 把不带版本的路径型开发依赖计为通配版本（见[CI 与质量门禁](05-ci-and-quality-gates.md)第 3 节）。

【事实】（2026-09-14，v0.1.0 发布记录）crates.io 对同一账号发布新 crate 有频率限制（策略见 `https://crates.io/docs/rate-limits`）：首次成功的运行连续上传 5 个 crate 后未再受阻；约 10 分钟后的运行只再上传 1 个（`ferrin-openai-compatible`），第 7 个（`ferrin-testing`）返回 `429 Too Many Requests`，正文为 “You have published too many new crates in a short period of time. Please try again after Mon, 14 Sep 2026 07:01:00 GMT”。该限制只针对新 crate；已存在 crate 的新版本另有更宽的限制。据此 `release.yml` 的发布步骤加入按服务器给出时间等待并重试的循环（见第 3 步）。

【事实】（2026-09-14，v0.1.0 发布完成）加入等待重试后的运行 run 34815430254（tag `v0.1.0`，提交 00e9b61）共 10 次尝试：第 1 次上传 `ferrin-testing` 后被限流，此后每次尝试上传 1 个新 crate 再被限流，服务器给出的放行时间依次为 07:01、07:11、…、08:21 GMT（间隔恒为 10 分钟，`date -u -d` 在 `ubuntu-24.04` 运行器上解析成功，等待 184–595 s），第 10 次上传 `ferrin` 后 15 个 crate 全部在 crates.io 可见，随后创建 GitHub Release `v0.1.0`（发布于 08:21:33Z，说明由 git-cliff 生成），运行总时长约 95 分钟。docs.rs 当日对 15 个 crate 的 0.1.0 全部构建成功（`/crate/<name>/0.1.0/status.json` 的 `doc_status` 为 `true`）。据此推断 crates.io 对新 crate 的限流约为每 10 分钟补充 1 个配额（`https://crates.io/docs/rate-limits` 未公布具体数值）；后续版本只涉及已存在的 crate，不受此限制。

## 7. 支持策略

- 只对最新次版本发布修复。
- 安全修复可对上一个次版本发布补丁。

## 8. 规范演进流程

`ferrin-spec` 的破坏性变更（新增必填字段、变更 trait 方法签名、变更序列化格式）需：

1. ADR（状态 `proposed`），说明动机、受影响的适配器、迁移步骤。
2. 在 `ferrin-testing` 中先更新契约检查与 Mock。
3. 同一 PR 或同一发布批次内更新全部第一方供应商 crate。
4. 变更日志的 `Changed` 段以 `SPEC:` 前缀标注。

【决策】Ferrin 不在运行时维护多个规范版本共存（不设并行的版本化接口与升级适配器），而以 crate 版本承载规范演进（[ADR 0011](../04-decisions/2026-09-13-0011-spec-versioning-by-crate-version.md)），因此规范变更的评审门槛更高，见 [CI 与质量门禁](05-ci-and-quality-gates.md)第 2 节第 11 条。

## 9. 0.1.1 发布（2026-09-15）

【事实】工作区清单与带版本号的内部依赖统一使用 0.1.1。根目录及全部 15 个 crate 的变更日志在 `[0.1.1] - 2026-09-15` 下记录本次变更，保留空的 `Unreleased` 段供后续工作使用（来源：`Cargo.toml`、`Cargo.lock` 和各 crate 变更日志）。这些发布文件不代表注册表上传成功；上传结果须由发布工作流与注册表记录另行验证。

【决策】在说明 [ADR 0019](../04-decisions/2026-09-15-0019-fallible-schema-transforms.md) 中的破坏性 Schema API 变化后，维护者明确选择并授权发布 0.1.1。本次发布是第 1 节兼容补丁规则的一次例外，版本号不能被理解为与 0.1.0 向后 API 兼容。调用方必须传播或处理 `Schema::transformed`、`SchemaTransform::apply`/`applied` 与 `to_openai_strict` 新增的 `Result`。后续发布仍遵循通用版本策略。

【事实】准备提交 `6b6d88b` 通过 [CI run 34944988012](https://github.com/f4tumnigrum/ferrin/actions/runs/34944988012) 的全部 14 个作业，包括三种平台上的测试与包验证。该运行早于本次带日期的发布文档修改，并未验证 0.1.1 的注册表上传或 docs.rs 构建。

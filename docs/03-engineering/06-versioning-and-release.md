# 版本与发布

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
3. `release.yml`（环境 `release`，secret `CARGO_REGISTRY_TOKEN`）先校验 tag 与版本、变更日志段，再按 `cargo xtask publish-order` 输出的顺序执行 `cargo publish -p <crate> --locked`；`cargo publish` 自身等待 crates.io 索引可见后返回；已发布的 crate（`cargo info` 可查到该版本）跳过，因此失败后可从中断点重跑。
4. 发布 GitHub Release：正文由 `git cliff --latest` 从上一 tag 以来的约定式提交生成，并附根 `CHANGELOG.md`。
5. docs.rs 构建检查：所有 crate `all-features` 文档构建成功。

【决策】（2026-09-14）首个版本用 crates.io API token（`CARGO_REGISTRY_TOKEN`）发布，工作流权限只保留 `contents: write`。依据：crates.io 的 Trusted Publishing 需要 crate 已存在并在 crates.io 上配置 GitHub 仓库为可信发布者，首次发布无法使用；首个版本发布后可改为 `rust-lang/crates-io-auth-action`（需 `id-token: write`），届时更新本节。

【事实】（2026-09-14）本地无法完整预演发布：本机 `~/.cargo/config.toml` 把 crates.io 替换为镜像源，`cargo package`/`cargo publish --dry-run` 对未发布的工作区内依赖（如 `ferrin-provider-util` → `ferrin-spec`）报“no matching package”，因为 Cargo 1.90 起的多包打包只对 crates.io 源叠加本地包；改用不含镜像配置的临时 `CARGO_HOME` 从仓库目录外调用可绕过：当日以此方式对 15 个可发布 crate 执行 `cargo package --locked -p …`（含从打包源码的验证构建）全部成功，约 6 分钟；`cargo publish --dry-run` 未另行执行（同一打包与验证步骤已覆盖，上传步骤只能在发布时验证）。CI 的 `package` 作业在无镜像的运行器上执行同一检查。

发布顺序（依赖拓扑）：`ferrin-spec` → `ferrin-schema` → `ferrin-message` → `ferrin-provider-util` → `ferrin-tool` → `ferrin-macros` → 供应商 crate、`ferrin-mcp` → `ferrin-core` → `ferrin-otel`、`ferrin-testing` → `ferrin`。

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

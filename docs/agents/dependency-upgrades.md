# Dependency Upgrades

依赖版本升级与工具链 bump 的操作纪律。触发条件见 `AGENTS.md` 的「开始前：按触发条件读文档」。

本文件只写**口径与纪律**，不复述可查事实：当前版本以 `src-app/Cargo.toml` 与锁文件（`src-app/Cargo.lock`）为真源，先例以 git 历史为真源。

Web 参照前端与其 pnpm 工具链已整体退役（ADR-0148）：本文件不再有 JS 依赖升级面；守门脚本类型检查的工具链三包（typescript / bun-types / @types/node）版本钉死在 `scripts/check-scripts-types.sh`，升级即改该脚本（缓存目录按版本串键控自动重建）。

## 口径：什么是「升到最新」

**「最新」= 最新 stable，不含预发布。** 三层分级，按顺序做、分开提交，保证可二分定位：

1. **锁文件刷新（必做）**：`cargo update`。把 semver 范围内可解析的最新版本落进锁文件。
2. **manifest 说明符写实（可选、纯维护）**：`^` 下限陈旧时写实到当前实际解析版本。**这不改变任何解析版本**，只让声明反映现实。
3. **跨 major 升级（逐个审）**：每个 major 单独体检，单独提交、单独回退点。

不抬 patch/minor 的 manifest 下限来「制造」升级——`#495` 先例只在跨 major 时改声明。若某包被传递依赖钉死，**原地不动**——抬下限改不动锁文件，只会让 manifest 说谎。

## 跨 major 体检

升 major 前必须**实测**，不接受「应该兼容」：

- 读上游官方迁移/破坏性变更清单，逐条对照本仓**实际用法**（配置文件、测试文件、导入面），标注「适用 / 不适用」并留证据。
- 适用项若导致存量代码变红，**修复单独一个提交**，与「依赖升级」本身分开——保持 PR 可二分。
- 有真实可跑的探针就实跑，别只做静态分析。「静态分析看着等效」不是证据。

## 刻意 hold 的写法

包被刻意留在旧 major 时，**必须在 manifest 注释里写明可验证依据**，不能只写结论：

- 具体失败模式（报错原文、退出码），不是「不兼容」三个字；
- 上游追踪 issue 号与状态；
- 解除条件（等什么、等到什么程度）。

反面教材：把 hold 写成「暂不支持」——下一个人（或 AI）会重新踩一遍。

## 工具链与 CI 固定值

- CI 固定值（bun 版本）与 `scripts/check.sh` 里的报错文案是**两处**，升级时同步改。
- 守门脚本类型检查工具链版本钉在 `scripts/check-scripts-types.sh`（单一事实源），升级走显式 bump。
- 工具链 bump 与依赖版本升级**分开做**：工具链牵动 CI 固定值与 ADR-0083 的约定，混在一起会让回退粒度失真。

## 验证

`scripts/check.sh` **不跑测试**，只跑类型检查、fmt 与各守门脚本。依赖升级必须额外跑：

- `cargo test --workspace --lib --test '*'`（Rust 三层：域单测 + 壳 headless 组件测试 + API 集成测试；glob 全量命中集成 target，与 build.yml 的 backend job 同一形态；`--workspace` 覆盖全部成员 crate）

并把结果与**升级前基线**对比（改动前先跑一遍基线，别拿升级后的数字自证）。

**发布面**：Linux 发布构建 workflow 仅 tag 或手动 dispatch 触发，PR 上不打包——本机能出的产物（release 二进制）尽力跑一次，其余在 PR 正文**显式声明未验证**，不得沉默略过。

## CHANGELOG

用户可见变化（运行时依赖、产物互通性）**必记**。纯 devDependency 与传递依赖刷新按 Keep a Changelog 字面纪律本可不记；本仓沿用 `#495` 先例记一条极简条目，保留审计痕迹。

## 提交与交付

- 先例：`#495`，单 PR + 分层 commit + 验证证据 + 可二分定位（前端分层历史；前端面已退役，现行只剩 Rust 侧分层）。
- 走标准链路：issue 认领 → 独立 worktree → 提交 → PR（`Closes #N`）→ 不自行合并。
- 不引入 Dependabot / Renovate：本仓 PR 须走 issue 认领 + worktree + 三层测试，机器人会持续产出不合流程的 PR。节奏靠人工——定期 `cargo update --dry-run` 并开 issue。

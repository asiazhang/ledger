# Dependency Upgrades

依赖版本升级与工具链 bump 的操作纪律。触发条件见 `AGENTS.md` 的「开始前：按触发条件读文档」。

本文件只写**口径与纪律**，不复述可查事实：当前版本以 `package.json` / `src-tauri/Cargo.toml` / 锁文件为真源，先例以 git 历史为真源。

## 口径：什么是「升到最新」

**「最新」= 最新 stable，不含预发布。** 三层分级，按顺序做、分开提交，保证可二分定位：

1. **锁文件刷新（必做）**：`pnpm update` + `cargo update`。把 semver 范围内可解析的最新版本落进锁文件。
2. **manifest 说明符写实（可选、纯维护）**：`^` 下限陈旧时写实到当前实际解析版本。**这不改变任何解析版本**，只让声明反映现实。
3. **跨 major 升级（逐个审）**：每个 major 单独体检，单独提交、单独回退点。

不抬 patch/minor 的 manifest 下限来「制造」升级——`#495` 先例只在跨 major 时改声明（`pinia ^3→^4`、`jsdom ^29→^30`），`typescript` 一行原封不动。若某包被传递依赖钉死（如 `@types/node` 被 `eval` 钉在 22.20.1），**原地不动**——抬下限改不动锁文件，只会让 manifest 说谎。

## 跨 major 体检

升 major 前必须**实测**，不接受「应该兼容」：

- 读上游官方迁移/破坏性变更清单，逐条对照本仓**实际用法**（配置文件、测试文件、导入面），标注「适用 / 不适用」并留证据。
- 适用项若导致存量代码变红，**修复单独一个提交**，与「依赖升级」本身分开——保持提交可二分。
- 有真实可跑的探针就实跑，别只做静态分析。「静态分析看着等效」不是证据。

## 刻意 hold 的写法

包被刻意留在旧 major 时，**必须在 manifest 注释里写明可验证依据**，不能只写结论：

- 具体失败模式（报错原文、退出码），不是「不兼容」三个字；
- 上游追踪 issue 号与状态；
- 解除条件（等什么、等到什么程度）。

反面教材：把 hold 写成「暂不支持」——下一个人（或 AI）会重新踩一遍。同理，**不用别名方案**（`typescript → npm:@typescript/typescript6` 之类）绕过 hold：那是把不确定性藏进依赖树，会让 `pnpm outdated` 与审计看不清真实状态。

## 工具链与 CI 固定值

- `packageManager`（pnpm）精确写死，升级走**显式 bump** 并重新生成锁文件，不做静默漂移（ADR-0046 决策 4）。
- CI 固定值（Node / bun 版本）与 `scripts/check.sh` 里的报错文案是**两处**，升级时同步改。
- CI 一律 `pnpm install --frozen-lockfile`（ADR-0046 决策 3）。
- 工具链 bump 与依赖版本升级**分开做**：工具链牵动 CI 固定值与 ADR-0046/0083 的约定，混在一起会让回退粒度失真。

## 易碎件：补丁依赖

`patches/` 下的补丁**绑定到具体版本**。升级对应依赖会让补丁失效，**必须重做并验证**：patch 应用失败时 `pnpm install` 会报错，但补丁「应用成功但语义已变」是静默的——需按补丁注释里的原始问题重新验证行为。当前受管补丁见 `pnpm-workspace.yaml` 的 `patchedDependencies`。

## 验证

`scripts/check.sh` **不跑测试**，只跑类型检查、lint、fmt 与各守门脚本。依赖升级必须额外跑：

- `pnpm test`（前端全量）
- `cargo test --workspace --lib --test '*'`（Rust 三层：域单测 + API 集成测试 + e2e BDD；glob 全量命中集成 target，与 build.yml 的 backend job 同一形态；`--workspace` 覆盖全部成员 crate）

并把结果与**升级前基线**对比（改动前先跑一遍基线，别拿升级后的数字自证）。

**打包盲区**：发布构建 workflow 仅 tag 或手动 dispatch 触发，合入 `main` 前不打包——DMG / APK 在交付阶段结构上无法验证。本机能出的产物尽力跑一次（`pnpm run tauri build`），其余在交付报告与 issue 评论**显式声明未验证**，不得沉默略过。

## CHANGELOG

用户可见变化（运行时依赖、产物互通性）**必记**。纯 devDependency 与传递依赖刷新按 Keep a Changelog 字面纪律本可不记；本仓沿用 `#495` 先例记一条极简条目，保留审计痕迹。

## 提交与交付

- 先例：`#495`，单 PR + 分层 commit（前端小版本 / 前端大版本 / 后端 crate）+ 验证证据 + 可二分定位。
- 走标准链路：issue 认领 → 独立 worktree → 提交（`Closes #N`）→ `/finish-worktree` 直合；依赖升级触及不可逆面，收尾报告须单独列出，打包盲区声明落在交付报告与 issue 评论。
- 不引入 Dependabot / Renovate：本仓改动须走 issue 认领 + worktree + 三层测试，机器人会持续产出不合流程的 PR。节奏靠人工——定期 `pnpm outdated` + `cargo update --dry-run` 并开 issue。

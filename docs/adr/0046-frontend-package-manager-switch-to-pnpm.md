# ADR-0046: 前端包管理器从 npm 切换至 pnpm——严格隔离守依赖边界、全量切换不留双锁文件

- 状态：已接受
- 日期：2026-08-31
- 作者：Ledger 项目
- 关联：issue #326（spec，parent）；实施票 #327（依赖层与锁文件）、#328（脚本与 CI 切换）、#329（文档与 ADR 同步）

## 背景

前端依赖原由 npm 统一管理（`package-lock.json` 锁文件、脚本与 CI 里的 `npm ci` / `npm run` / `npx`）。三个问题推动切换：安装慢；扁平 `node_modules` 磁盘占用大；更重要的是扁平结构会**静默掩盖「幽灵依赖」**——代码 import 了未显式声明的传递依赖时仍能编译，删掉某个显式依赖后换一种安装方式就立刻崩，依赖边界失守全凭自觉。需要一个更严格、更快的包管理器，并把整条工具链（本地脚本、Tauri 引导命令、CI、文档）统一到它上面，避免双包管理器并存的锁文件分叉。

## 决策

1. **全量切换至 pnpm 12，不留双包管理器并存**：锁文件、本地脚本、Tauri 引导命令、CI、文档一并切换。半吊子迁移（CI 与本地各锁一套依赖树）比不迁移更糟。pnpm 12 是 Rust 重写版，命令 / flag / 设置 / 锁文件格式与 11 兼容，属「非迁移」升级；`latest` dist-tag 仍指向 11 线，pnpm 12 走 `next-12` tag，当前以 `12.1.0` 为稳定锚点。
2. **选型动机 = 幽灵依赖失守 + 安装慢 / 磁盘大**：pnpm 的严格 `node_modules`（软链进全局 content-addressable store）在代码 import 未显式声明的依赖时立即报错，依赖边界由工具强制守住而非靠自觉；全局 store + 硬链接让安装更快、新克隆与 worktree 秒级就绪、多检出复用同一份依赖、磁盘占用更小。
3. **锁文件：全新解析生成，唯一事实**：用 `pnpm install` 按 manifest 的 semver range 重新解析生成 `pnpm-lock.yaml`（非忠实转换旧 `package-lock.json`），接受 `^` 依赖的版本漂移——有意使用更新版本；随后删除 `package-lock.json`，`pnpm-lock.yaml` 成为唯一锁文件。CI 一律 `pnpm install --frozen-lockfile`（或等价开关），对应原 `npm ci` 的严格可复现语义，锁文件与 manifest 不一致即失败。
4. **版本固定：`packageManager` 精确写死**：根 package manifest 加 `"packageManager": "pnpm@12.1.0"`（精确版本，不用 `^`）。锁文件与包管理器版本强相关，跟进大版本走显式 bump，不做静默漂移。
5. **构建脚本放行：workspace 配置 `allowBuilds`**：pnpm 默认拒绝执行未放行的依赖构建脚本（未放行时报 `ERR_PNPM_IGNORED_BUILDS`）。唯一带安装脚本的依赖是 `fsevents`（macOS 可选依赖，Vite 文件监听所需），在 workspace 配置里 `allowBuilds: { fsevents: true }` 显式放行；Linux / CI 不安装 fsevents，不受影响。pnpm 12 已弃用 `onlyBuiltDependencies`，放行改走 `allowBuilds`。
6. **命令引用统一替换规则**：本地脚本 `npm run X` → `pnpm run X`；项目内二进制 `npx <bin>` → `pnpm exec <bin>`（exec 严格命中本项目锁定的版本，不用 dlx 拉取外部包）；Tauri 配置的 `beforeDevCommand` / `beforeBuildCommand` 改 `pnpm run`。README 安装 / 构建命令、源码注释同步为 pnpm 语境。
7. **CI 安装动作：官方继任者 `pnpm/setup`**：用 `pnpm/setup`（v2+）替代 `pnpm/action-setup` 与 `actions/setup-node`——它安装 pnpm v11+ 原生二进制、可在同一步装 Node 运行时（省一个独立步骤与一处版本漂移来源）、自动执行 `pnpm install`、支持 `require-lockfile` 跑 frozen-lockfile 语义。`pnpm/action-setup` 已声明此继任者。
8. **幽灵依赖处理策略：显式声明，不放松隔离**：pnpm 严格隔离暴露「直接 import 未显式声明依赖」时，以显式声明该依赖的方式修复。迁移验收触发点是「严格隔离下质量门全绿」而非「pnpm 跑通」——本次迁移验证：vue-tsc 类型检查、前端单测、vite 生产构建在严格隔离下全部通过，无幽灵依赖暴露。
9. **顺带结构变化：引入 workspace 配置**：`allowBuilds` 写在 workspace 配置层，仓库根即 workspace root，新增一份 workspace 配置文件——这是为放行构建脚本引入的最小结构变化。
10. **不改动 Rust 侧与 AI 导入域**：cargo 依赖管理与本变更无关；本地 HTTP API 与领域逻辑不受影响。

## 理由

- **为什么全量切换而非双包管理器并存**：CI 与本地各锁一套依赖树是比不迁移更糟的状态——「npm 引导 + pnpm 安装」的错位会让依赖树的生成与消费方不一致。
- **为什么全新解析而非忠实转换旧锁文件**：`^` range 下的版本漂移是有意使用更新版本，避免把 npm 时代的旧依赖树固化进新包管理器；旧锁文件只描述「曾解析出什么」，不承载选型意图。
- **为什么 `packageManager` 用精确版本**：锁文件格式与包管理器版本强相关，`^` 会让本地与 CI 解析到不同 pnpm 版本而锁出不同树；精确写死使升级成为一次显式动作。
- **为什么项目内二进制用 exec 而非 npx / dlx**：exec 走本项目锁定的依赖树，严格命中 `package.json` 声明版本，不落入全局缓存或临时拉取的同名工具。
- **为什么放行走 `allowBuilds` 而非 `onlyBuiltDependencies`**：pnpm 12 已弃用后者，`allowBuilds` 是当前语义；放行对象刻意收窄到唯一带安装脚本的 `fsevents`，其余依赖的构建脚本仍被默认拒绝。
- **为什么幽灵依赖以「显式声明」修复而非放松隔离**：放松隔离等于回到 npm 的依赖边界失守状态——迁移收益本身就是严格隔离，验收触发点因而定在「幽灵依赖暴露为零」。

## 代价与边界

1. `^` 依赖相对旧 `package-lock.json` 有版本漂移（有意为之，非回归）；后续依赖升级仍按 semver 自然进行。
2. 依赖安装脚本一律默认拒绝，未来新增带安装脚本的依赖需在 `allowBuilds` 显式放行（防呆默认，符合安全惯例）。
3. 包管理器版本升级必须显式 bump `packageManager` 并重新生成锁文件——多一步操作，换来本地与 CI 的可复现一致。
4. 本决策只定工具链取舍，不改变既有质量门内容：`scripts/check.sh`（vue-tsc + clippy + fmt + 文档一致性检查）与 CI 仍是验收基准，不新增单元测试（无运行时行为可测）。
5. 不引入 Node 版本管理器（nvm / volta / mise），仅固定包管理器版本；CI 的 Node 运行时版本由 `pnpm/setup` 的 `runtime` 参数承载，版本号语义对齐原 `setup-node` 配置。
6. 本变更是内部工具链切换，不产生面向终端用户的可见变更，不写 CHANGELOG 用户条目（无 `pnpm` 引用残留导致的功能回归为前提）。

## 相关 ADR

- 无领域关联 ADR（纯工具链决策，不涉及领域词汇表）。开发者工具链说明见 AGENTS.md「工作流约定」与 README「从源码构建」。

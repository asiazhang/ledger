# ADR-0083: 守门脚本 TypeScript 化、Bun 运行时与挂载登记单源

- 状态：已接受（改判 ADR-0047「Node，无依赖」中的运行时表述为 Bun；正文已按 #1149、#1682/#2204、#1991/#1995 现行态收敛）
- 日期：2026-09-06（现行态收敛：2026-11-21）
- 作者：Ledger 项目
- 关联：ADR-0046（pnpm 包管理）；ADR-0047（命令注册守门，运行时表述由本 ADR 改判）；ADR-0049 / ADR-0056 / ADR-0071 / ADR-0087（各守门脚本与断言强度）；ADR-0145（守门注释清零与数据面收口）；#734 / #737 / #1149 / #1682 / #1991 / #1995 / #2204

## 背景

仓库守门脚本（`scripts/check-*.ts`）承载全仓关键的白名单、词法掩码与边界扫描规则。早期使用无类型 JS 靠 Node 运行，类型错误只能运行时暴露，测试文件也缺少类型检查；同时 `scripts/*.test.ts` 一度由 Vitest 在 Node 下收集却再 `spawnSync('bun')`，且质量门在 `check.sh` 与 CI workflow 中的挂载缺乏单一事实源，容易静默漏挂。

## 决策

### 1. 守门脚本全量 TypeScript 化，零 npm 运行时依赖

- 所有守门脚本以 `.ts` 编写，仅使用 `node:` 内置模块（由 Bun 内建兼容层实现），保持零 npm 运行时依赖；`@types/node` 与 `bun-types` 仅为 devDependency 服务类型检查。

### 2. 双运行时严格分治（#1991 / #1995）

- **门槛脚本与守门测试 = Bun（固定版本 1.4.0）**：脚本执行方式为 `bun scripts/check-*.ts`；全部守门测试 `scripts/*.test.ts` 的**唯一运行口径为 `bun test scripts/`**，测试文件显式 `from "bun:test"`（消除隐式 `"vitest"` 别名依赖），仍经 `spawnSync('bun', …)` 覆盖真实门槛路径。本地 `check.sh` 缺失 `bun` 时显式报错退出，绝不静默降级回 Node。
- **前端构建与前端测试 = Node@22**：`vite` / `vitest` / `vue-tsc` 等前端工具链跑在 Node；Vitest 收集范围不含 `scripts/`，前端测试与 CI `frontend-test` 分片均不依赖 `bun` 在场。
- **改判 ADR-0047**：保留「零 npm 依赖」，运行时由 Node 改判为 Bun。

### 3. 独立类型检查工程（`tsconfig.scripts.json`）

- 公共 `compilerOptions` 与根工程共用 `extends` 源 `tsconfig.base.json`（#1149），独立工程的 `types` 显式包含 `node`、`bun-types`（精确 1.4.0）与 `vitest/globals`（服务测试助手与部分测试全局）；由 `vue-tsc` 执行，挂入 `check.sh` 与 CI `frontend` job。

### 4. 质量门挂载登记单源（`scripts/gate-mounts.ts`，#1682 / #2204）

质量门「哪些门被跑、挂在哪个宿主」的单一事实源住 `scripts/gate-mounts.ts`（`GATE_MOUNTS`），而非靠 `check.sh`、CI workflow 与注释三处手写对账：
- **判定语义 = per-host 行首前缀**：宿主文件非注释行 trim 后行首匹配登记前缀，天然免疫 `echo "▶ …"` 展示行假绿，且显式表达 `check.sh` 与 CI 步骤间的命令形态差异。
- **核对而非生成**：`check.sh` 与 CI workflow 保持手写，由 `gate-mounts.test.ts` 断言双向全等——删任一宿主挂载行或删任一条登记至少一条测试变红（ADR-0087）。新增守门须同步在 `GATE_MOUNTS`、`check.sh` 与 CI 挂载。

## 备选与否决

- **Node `--experimental-strip-types`**：erasable-only 子集受限且版本门槛高，否决。
- **`tsx` devDependency**：破坏守门脚本零 npm 依赖自持性，否决。
- **保持 `.js` + JSDoc**：类型表达力不足且补不上测试助手类型缺口，否决。
- **由 Vitest 收集并运行 `scripts/*.test.ts`**：造成前端测试套件强依赖 `bun` 在场，制造双运行时灰色地带，已于 #1991 淘汰。

## 后果

- 本地开发机与 CI `frontend` job 需安装固定版本 Bun（1.4.0），缺失即显式报错；CI `frontend-test` 分片零 Bun 依赖。
- 守门测试经真进程 `spawnSync` 覆盖外部退出码与输出契约，全量 `bun test scripts/` 与 `gate-mounts.test.ts` 守住质量门槛不漏挂、不漂移。

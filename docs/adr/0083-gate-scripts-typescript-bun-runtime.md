# ADR 0083: 守门脚本 TypeScript 化与 Bun 运行时——零 npm 依赖保留、类型检查入门槛（收窄范围）、门槛/前端双运行时

- 状态：已接受
- 日期：2026-09-06
- 作者：Ledger 项目
- 关联：issue #734（spec，grilling 定稿）、#737（实施收窄票，#738–#740 为后续扩量票）

## 背景

`scripts/` 下四个守门脚本（check-commands / check-structure / check-i18n-keys /
check-test-stubs）为无依赖 ESM JavaScript（仅 `node:` 内置模块），以 node 运行。诉求：
强类型化——守门脚本维护着全仓最关键的文本扫描规则（白名单、正则、掩码），类型错误只能
靠运行时暴露。同时补上另一处缺口：测试文件与测试助手从未进入任何类型检查工程（根
tsconfig 只覆盖 `src` 非测试代码）。

运行时候选三选一（grilling 定稿）：① node 原生 strip-types——受 erasable-only 约束，
TS 语法子集受限且版本门槛高；② tsx devDep——给「零 npm 依赖」的守门脚本引入运行时依赖，
违背其自持性质；③ Bun——原生跑 `.ts`、`node:` 兼容层覆盖本仓用到的全部内置模块、单二进制
零 npm 依赖。取 ③。

## 决策

1. **四脚本全量迁 `.ts`，行为零变化**：校验规则、白名单、扫描边界、输出文案逐字保留；
   实跑核对四个脚本 node+`.js`（迁移前）与 bun+`.ts`（迁移后）stdout/stderr/退出码逐字
   一致。零 npm 依赖性质保留：脚本 import 仅 `node:` 内置模块（bun 内建兼容层实现），
   不新增任何运行时依赖。`@types/node` 是 devDependency，只服务类型检查（门槛期依赖），
   非运行时依赖。shebang 同步为 `#!/usr/bin/env bun`。

2. **守门脚本运行时 = Bun（CI 固定 1.4.0）**：调用方式 `bun scripts/check-*.ts`。四个
   守门测试（check-*.test.ts）以 `spawnSync('bun', …)` 同运行时调用——测的就是门槛路径；
   check-structure.test.ts 对脚本的静态导入改 `.ts` 扩展名。本地 `check.sh` 前置
   `command -v bun` 检测：缺失即显式中文报错退出（指向本 ADR 与安装方式），不静默降级
   回 node——降级会把「门槛在什么运行时上跑」变成机器相关的偶然事实。

   > 与 ADR-0047 决策 4/5 的「Node，无依赖」表述冲突——按规则显式处理：**零 npm 依赖
   > 性质保留不变，运行时由 node 改判为 Bun**（本 ADR 为运行时口径的唯一解释处）；
   > ADR-0047 的坐标已同步指向此处。

3. **类型检查入质量门槛（`tsconfig.scripts.json`）**：独立 TS 工程，compilerOptions 与
   根 tsconfig 同构（strict 套件 / bundler 解析 / DOM lib），差异两处——`types` 限定
   `node` + `vitest/globals`（测试全局经 globals:true 使用，类型声明须显式引入）；
   include 收窄为「脚本目录 + 测试助手 + 守门相关测试 + src 全局 `.d.ts`」。执行器用
   vue-tsc（与前端类型检查同一执行器，为后续扩量保持命令面不变）；挂入 `check.sh`
   （紧跟前端类型检查之后）与 CI frontend job 各一步。

4. **门槛范围收窄（#737 定稿，先绿后扩）**：存量测试文件存在大量类型错（`.vue` 导入的
   组件类型、手搓 invoke mock 的参数窄化），一次性纳入不可行。本票范围 = `scripts/**` +
   `src/__tests__/helpers/**` + 四个 check-*.test.ts + 全局 `.d.ts`，门槛先绿；扩到
   全量测试目录由 #738–#740 分批承接。为此新增两个测试助手收口 mock 边界：
   `helpers/invoke-mock.ts`（tauri invoke mock 单一入口，单点把 `InvokeArgs` 联合收窄为
   对象形态，替代全仓测试散布的 `vi.mocked(invoke)` + as 断言）与
   `helpers/component-vm.ts`（`findComponent` 字符串选择器实例窄化单点）。

5. **双运行时事实固化**：**门槛脚本 = Bun；前端构建/测试（vite / vitest / vue-tsc /
   build.rs 之外的 node 工具）= node@22**。两套运行时各管一段，不存在「哪边都能跑」的
   灰色地带；CI frontend job 与 frontend-test 分片 job 均经 `oven-sh/setup-bun@v2`
   安装固定 1.4.0（与本地门槛同一固定值，升级走显式 bump），其余步骤不变。

6. **脚本坐标全量同步**：AGENTS.md、ADR-0047/0049/0056/0071、`src-tauri/build.rs`、
   `src-tauri/src/signals_cross_check.rs`、i18n 模块注释、测试头注释中的
   `check-*.js` 坐标全部改指 `.ts`；全仓 `rg` 零 `.js` 旧坐标残留（守门脚本相关）。
   机制术语「守门脚本 / 门槛脚本」沿用各 ADR 既有定义，不进词汇表。

## 理由

- **为什么独立 tsconfig 而非扩根 tsconfig**：测试代码的类型现状（68 处 `.vue` 导入错）远
  未达门槛水位，混入根工程会把前端类型检查一起拖红；独立工程让「脚本 + 助手 + 守门测试」
  先达标，扩量不回灌前端检查。
- **为什么 spawnSync('bun') 而非 import 被测函数**：守门测试按「只测外部可观察结果」
  决策（ADR-0047 决策 5 先例）以退出码与输出为接缝；bun 调用与 check.sh / CI 门槛调用
  同款，测试覆盖的就是真实门槛路径。例外：check-structure.test.ts 另静态导入
  `WHITELIST`/`LAYER` 派生夹具清单（单一事实源，#725 先例）。
- **为什么版本钉死 1.4.0 而非浮动**：守门脚本属质量地基，运行时升级应是一次显式评审
  （bun 的 node: 兼容层仍在快速演进），CI 与本地同钉一版消除「本地绿 CI 红」的运行时
  漂移面。
- **被排除路线（防重提）**：① node `--experimental-strip-types`——erasable-only 约束
  排除 enum/namespace/参数属性等形态，且版本门槛与 LTS 现实不符；② tsx——运行时
  devDep，破坏守门脚本零依赖自持性；③ 保持 .js + JSDoc——类型表达力不足，测试类型
  缺口同样补不上。

## 代价与边界

1. 开发机与 frontend-test CI job 从此依赖 bun：无 bun 时门槛显式报错（check.sh 中文提示）
   或守门测试 spawn 失败——失败面显式，不存在静默路径。
2. `tsconfig.scripts.json` 的 include 是当前收窄范围而非终态：扩量由 #738–#740 承接，
   翻转完成前全量测试的类型缺口仍在（本 ADR 记录的是门槛机制，不含扩量承诺）。
3. bun 对 `node:` 内置的实现与 node 存在行为差异的可能：脚本只依赖
   fs/path/url/child_process 的稳定子集，且输出逐字一致性已实跑核验；bun 升级时重跑
   一致性核对即可。
4. 守门测试经 spawnSync 起真进程，比纯内存测试慢（四个测试文件 ~1s）；外部可观察接缝
   的收益优先于这层耗时。

## 相关 ADR

- ADR-0047（命令注册单一来源）：check-commands 的宿主决策；本 ADR 改判其运行时表述
  （Node → Bun），校验机制与扫描边界不变。
- ADR-0049（应用 i18n）：check-i18n-keys 的宿主决策。
- ADR-0056（后端域目录化）：check-structure 的宿主决策。
- ADR-0071（派生缓存归域）：check-structure infra→域反向扫描的扩展决策。
- ADR-0046（包管理器切 pnpm）：CI frontend job 执行器即 pnpm；bun 与 pnpm 在同一 job
  并存，各管门槛脚本与依赖安装/前端构建。

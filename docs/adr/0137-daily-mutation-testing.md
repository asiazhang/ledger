# ADR-0137: 变异测试——本地按需运行与「测试有效性」的自动化判定口径

- 状态：已接受（CI 每日 workflow `mutation-bench.yml` 已于 2026-10-14 撤下，改为本地按需运行；正文已按 #1832 / #1935 与载体撤下现行态收敛）
- 日期：2026-09-25（现行态收敛：2026-10-14）
- 作者：Ledger 项目
- 关联：ADR-0087（断言强度——本 ADR 为其自动化投影）；ADR-0068（先观察后立阈的治理路径）；ADR-0112（crate 边界）；#1816（spec）、#1817/#1818（试点）、#1827、#1832（前端 Stryker 补丁与金丝雀）、#1935（白名单扩面诊断）；词汇表：测试基础设施域「零击杀测试」「存活变异体」「等价变异体」

## 背景

靠人工审阅判断「某条断言是否还有可能变红」（ADR-0087 断言强度）成本高且随演进漂移。引入变异测试工具——Rust 侧 `cargo-mutants` 与前端侧 `Stryker`——向源码注入受控变异并运行测试，可将断言强度投影为可执行的击杀信号。

本方案最初尝试在 GitHub Actions 以每日 cron（`mutation-bench.yml`）常驻运行，但在真实运行中证明托管 runner 成本不可行（上线 16 连败、仅批次 1 白名单已撞 180 分钟上限、全 workspace 约 6,197 变异体估算 124 小时）。因此撤下 CI 载体，保留整套判定口径、工具配置、豁免收口与本地按需运行纪律。

## 决策

### 1. 运行载体：本地按需运行（不进 `check.sh`、不进 CI）

- 变异测试不进 `check.sh` 与 PR 门禁（热回路保持秒级），也不设 CI 每日 workflow（托管 runner 成本不可行）。
- 改为**本地按需运行**，配合 `scripts/mutation-summary.sh` 与 `scripts/mutation-frontend-summary.sh` 在 stdout 阅读报告。

### 2. Rust 侧范围：白名单渐进扩面 + 按需 `--in-diff`

- 显式传 `--workspace --test-workspace=true`（`cargo mutants` 默认只变异根包，不传会静默漏掉全部 `crates/` 域代码，#1935 事故教训），按需配合 `--in-diff <diff>` 仅变异增量改动。
- 采用 `--file` 白名单渐进扩面：批次 1 覆盖 `crates/transaction/src/**`（资金写入核心）+ 根包 `src/commands/**` / `src/api_server/**`（命令出口接缝）；`src/bin` 与 `src/test_support` 长期排除；扩面需先消化上批存活分布。

### 3. 前端侧 Stryker：补丁适配 Vitest 5 + 金丝雀兜底（#1832）

- 覆盖 `src/**/*.ts`（不含 `.vue`），`stryker.config.mjs` 设 `dryRunTimeout: 900_000`（默认 300s 会在全量套件干跑时超时）。
- **Vitest 5 适配补丁**：`vitest-runner@10.0.0` 按空格拼用例名，而 Vitest 5 改为 `' > '` 分隔符导致 covered mutants 100% 误报 Survived；通过 `pnpm patchedDependencies` 套用上游修复（绑定精确版本 10.0.0），待上游正式发布适配版本后移除补丁。
- **金丝雀验收**：升级 `vitest-runner` 或 `Vitest` 前必跑 `src/stores/sidebar-order.ts` 单文件独立无缓存变异（`isArrangeableView` 必 Killed），防止误报静默复发；依赖升级纪律见 `docs/agents/dependency-upgrades.md`。

### 4. 观测期不设强阈值；判定口径三分类

不以存活变异体退出码作硬阻断（先积累分布与清理历史欠账），报告驱动三类处置：
- **零击杀测试**（对全量变异体无一击杀）→ 删除候选（等价于无任何实现改动可使其变红）。
- **击杀集被完全包含**的冗余测试 → 列清单交人工裁决，不自动删（保留回归定位粒度）。
- **存活变异体** → 不作为删除测试的依据；按域补测或登记豁免（等价/不可测变异体）。

### 5. 豁免单点收口：`src-tauri/.cargo/mutants.toml`

Rust 侧变异豁免统一收口在 workspace 根 `src-tauri/.cargo/mutants.toml`，写明豁免类别（等价变异体 / 不可测变异体）与理由，空清单起步按需追加。

## 备选与已退役方案

- **每日 CI workflow（`mutation-bench.yml`，已撤下）**：16 连败 + 180 分钟上限实证托管 runner 无法承载批次 1 及后续扩面；已删除 workflow，保留本地脚本与口径。
- **进 `check.sh` / PR 门禁**：单次耗时数十分钟至数小时，违背体验取舍标尺，否决。
- **全量每日跑**：全 workspace 6,197 变异体约 124 小时，不可行，否决。
- **降级 Vitest 4.x 换 Stryker 兼容**：牵动全部前端测试配置，为一个辅助工具降级测试栈不划算，否决。
- **按 CONTEXT 上下文切分 / 自动开 tracking issue**：与 crate 边界错位、或制造空 issue 噪音，否决。

## 后果

- 测试有效性获得可执行、可复现的本地量化手段，但缺少 CI 定时提醒的覆盖缺口需由本地开发与重构纪律承接。
- 补丁绑定 `vitest-runner@10.0.0`，升级测试栈时必须按金丝雀步骤核验报告可信度。
- 词汇表保留「零击杀测试」「存活变异体」「等价变异体」三术语。

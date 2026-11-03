// 前端渲染每日基准（issue #2012，关联 #2010）：与计数守门（render-gate.test.ts）
// 消费同一 fixture 与桌面表格装配，量「一次完整挂载的 JS 层成本」（组件实例化 +
// vnode + patch + DOM 构建，Vitest + jsdom、无 GPU 依赖）——N 次迭代取 p95
// （最近秩法，与 ledger-perf 同口径），**只记录不判定**（先例：bench-import /
// bench-sync / bench-market 观测项；基线稳定后另立阈值，不与首版绑定）。
// 与计数 gate 的分工：PR CI 靠计数 gate 拦「形态劣化」（确定性、必跑，
// render-gate.test.ts）；本基准靠耗时趋势捕获「计数之外的成本增长」（依赖升级、
// 框架行为变化），不进 PR 阻塞路径。挂载口径刻意不量 unmount（挂载是翻页渲染的主
// 成本面，卸载是常数收尾）。
//
// 运行口径：独立配置 `vitest.bench.config.ts`（默认套件不含本文件——include 只收
// *.test.ts，本文件 .bench.ts 后缀脱离默认收集；perf-bench.yml 以
// `pnpm exec vitest run --config vitest.bench.config.ts` 显式运行，报告行以
// `RENDER_BENCH` 前缀写入 stdout，CI 提取进 $GITHUB_STEP_SUMMARY）。

import { it, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";
import {
  FIXTURE_DISPLAY_ROW_COUNT,
  mountDesktopRenderTable,
  wireRenderFixture,
} from "./render-fixture";

vi.mock("vue-router", () => ({
  useRoute: () => ({ query: {} }),
  useRouter: () => ({ push: () => {} }),
}));

/** 迭代数：与 ledger-perf 查询基准同款真分位数口径（n < 20 时 p95 = max 无分辨力）。 */
const ITERATIONS = 20;
/** 预热迭代：JIT / 模块求值 / naive-ui 主题装配的首轮噪声不进统计。 */
const WARMUP = 2;

it(
  "桌面表格挂载基准（50 行 fixture，mount p95，只记录不判定）",
  // 基准单用例含 20 次迭代挂载，预算独立于默认套件的 30s 单测墙钟
  { timeout: 240_000 },
  async () => {
    await wireRenderFixture();
    const samples: number[] = [];
    for (let i = 0; i < WARMUP + ITERATIONS; i++) {
      const start = performance.now();
      const wrapper = mountDesktopRenderTable();
      const mountMs = performance.now() - start;
      // 微任务收尾与卸载在量测窗口外（挂载同步渲染已完成）
      await flushPromises();
      await wrapper.unmount();
      if (i >= WARMUP) samples.push(mountMs);
    }
    samples.sort((a, b) => a - b);
    // 最近秩法 p95（与 ledger-perf bench 同口径）
    const p95 = samples[Math.ceil(0.95 * samples.length) - 1];
    const min = samples[0];
    const avg = samples.reduce((s, v) => s + v, 0) / samples.length;
    console.log(
      `RENDER_BENCH suite=transactions-desktop-table display_rows=${FIXTURE_DISPLAY_ROW_COUNT} ` +
        `iterations=${ITERATIONS} warmup=${WARMUP} ` +
        `mount_ms_min=${min.toFixed(1)} mount_ms_avg=${avg.toFixed(1)} mount_ms_p95=${p95.toFixed(1)}`,
    );
  },
);

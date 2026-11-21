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

const ITERATIONS = 20;
const WARMUP = 2;

it("桌面表格挂载基准（50 行 fixture，mount p95，只记录不判定）", { timeout: 240_000 }, async () => {
  await wireRenderFixture();
  const samples: number[] = [];
  for (let i = 0; i < WARMUP + ITERATIONS; i++) {
    const start = performance.now();
    const wrapper = mountDesktopRenderTable();
    const mountMs = performance.now() - start;
    await flushPromises();
    await wrapper.unmount();
    if (i >= WARMUP) samples.push(mountMs);
  }
  samples.sort((a, b) => a - b);
  const p95 = samples[Math.ceil(0.95 * samples.length) - 1];
  const min = samples[0];
  const avg = samples.reduce((s, v) => s + v, 0) / samples.length;
  console.log(
    `RENDER_BENCH suite=transactions-desktop-table display_rows=${FIXTURE_DISPLAY_ROW_COUNT} ` +
      `iterations=${ITERATIONS} warmup=${WARMUP} ` +
      `mount_ms_min=${min.toFixed(1)} mount_ms_avg=${avg.toFixed(1)} mount_ms_p95=${p95.toFixed(1)}`,
  );
});

it("桌面表格挂载基准（100 行/页档位，mount p95，只记录不判定）", { timeout: 360_000 }, async () => {
  await wireRenderFixture();
  const samples: number[] = [];
  for (let i = 0; i < WARMUP + ITERATIONS; i++) {
    const start = performance.now();
    const wrapper = mountDesktopRenderTable({ txnCount: 100 });
    const mountMs = performance.now() - start;
    await flushPromises();
    await wrapper.unmount();
    if (i >= WARMUP) samples.push(mountMs);
  }
  samples.sort((a, b) => a - b);
  const p95 = samples[Math.ceil(0.95 * samples.length) - 1];
  const min = samples[0];
  const avg = samples.reduce((s, v) => s + v, 0) / samples.length;
  console.log(
    `RENDER_BENCH suite=transactions-desktop-table-100 display_rows=${FIXTURE_DISPLAY_ROW_COUNT * 2} ` +
      `iterations=${ITERATIONS} warmup=${WARMUP} ` +
      `mount_ms_min=${min.toFixed(1)} mount_ms_avg=${avg.toFixed(1)} mount_ms_p95=${p95.toFixed(1)}`,
  );
});

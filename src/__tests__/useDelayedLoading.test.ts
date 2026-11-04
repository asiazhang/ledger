import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
// 假时钟不冲刷微任务：watcher 为 pre-flush（微任务队列），推进一律用异步变体
// （advanceTimersByTimeAsync 边推进边冲刷），同步断言前先 await 让边沿落位。
import { ref } from "vue";
import { withSetup } from "@ledger/test-support/mount";
import { useDelayedLoading } from "@/composables/useDelayedLoading";

/** 延迟出场加载指示整流单测（issue #2010 翻页体验收尾）：快加载全程不可见、
 * 慢加载延迟登场、最小驻留补足、连续请求边沿独立、作用域销毁清理定时器。 */

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

describe("useDelayedLoading", () => {
  it("快加载（< showAfter 完成）：指示器全程不可见（零闪烁，#2010 翻页主场景）", async () => {
    const shell = withSetup(() => {
      const loading = ref(false);
      const visible = useDelayedLoading(loading);
      loading.value = true;
      void vi.advanceTimersByTimeAsync(50); // 50ms 后加载完成（快于出场延迟）
      loading.value = false;
      void vi.advanceTimersByTimeAsync(1000);
      return visible;
    });
    await vi.advanceTimersByTimeAsync(2000);
    expect(shell.value).toBe(false); // 从未登场
  });

  it("慢加载：showAfter 后登场，加载完成后随下降沿立即隐去（驻留已足）", async () => {
    const shell = withSetup(() => {
      const loading = ref(false);
      const visible = useDelayedLoading(loading);
      return { loading, visible };
    });
    shell.loading.value = true;
    await vi.advanceTimersByTimeAsync(119);
    expect(shell.visible.value).toBe(false);
    await vi.advanceTimersByTimeAsync(1); // = showAfter(120)
    expect(shell.visible.value).toBe(true);
    await vi.advanceTimersByTimeAsync(600); // 加载持续 600ms（≥ minShow 250）
    shell.loading.value = false;
    await vi.advanceTimersByTimeAsync(1);
    expect(shell.visible.value).toBe(false); // 驻留已足，下降沿即隐
  });

  it("临界慢加载（刚登场即完成）：补足 minShow 再隐去（过场完整淡出）", async () => {
    const shell = withSetup(() => {
      const loading = ref(false);
      const visible = useDelayedLoading(loading);
      return { loading, visible };
    });
    shell.loading.value = true;
    await vi.advanceTimersByTimeAsync(130); // 登场
    shell.loading.value = false; // 立刻完成（驻留 0/250）
    await vi.advanceTimersByTimeAsync(0);
    expect(shell.visible.value).toBe(true); // 仍显示（补驻留）
    // 驻留窗 = minShow −（下降沿 − 登场）= 250 − (130−120) = 240ms：窗内仍显示、窗后隐去
    await vi.advanceTimersByTimeAsync(239);
    expect(shell.visible.value).toBe(true);
    await vi.advanceTimersByTimeAsync(1);
    expect(shell.visible.value).toBe(false);
  });

  it("加载完成于 showAfter 边界前后翻转：未登场则零痕迹（无出场定时器泄漏）", async () => {
    const shell = withSetup(() => {
      const loading = ref(false);
      const visible = useDelayedLoading(loading);
      return { loading, visible };
    });
    shell.loading.value = true;
    await vi.advanceTimersByTimeAsync(119); // 尚未到出场
    shell.loading.value = false;
    await vi.advanceTimersByTimeAsync(5000); // 出场定时器应已取消
    expect(shell.visible.value).toBe(false);
  });

  it("串行请求边沿独立：第一轮未登场即完成，第二轮照常整流", async () => {
    const shell = withSetup(() => {
      const loading = ref(false);
      const visible = useDelayedLoading(loading);
      return { loading, visible };
    });
    // 第一轮：快加载零痕迹
    shell.loading.value = true;
    await vi.advanceTimersByTimeAsync(60);
    shell.loading.value = false;
    await vi.advanceTimersByTimeAsync(1);
    expect(shell.visible.value).toBe(false);
    // 第二轮：慢加载正常登场
    shell.loading.value = true;
    await vi.advanceTimersByTimeAsync(200);
    expect(shell.visible.value).toBe(true);
    await vi.advanceTimersByTimeAsync(1000);
    shell.loading.value = false;
    await vi.advanceTimersByTimeAsync(300);
    expect(shell.visible.value).toBe(false);
  });

  it("immediate 接线：挂载时已在加载（恢复访次首刷）也走出场延迟", async () => {
    const shell = withSetup(() => {
      const loading = ref(true);
      const visible = useDelayedLoading(loading);
      return { visible };
    });
    expect(shell.visible.value).toBe(false); // 挂载即加载不立即显示
    await vi.advanceTimersByTimeAsync(120);
    expect(shell.visible.value).toBe(true);
  });
});

import {
  setTxnDb,
  makeTxn,
  mountView,
  mountMobile,
  mountPhone,
  listCalls,
  lastListFilter,
  tablePagination,
} from "./common";
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { createOverlayToken, resetOverlays } from "@ledger/ui-kit/overlayRegistry";
import { NButton } from "naive-ui";
import type { Transaction } from "@ledger/types";
import { matchPeriodStepShortcut } from "@/composables/useTimePeriodShortcuts";

const HINT_ZH = ", 上一个周期 · . 下一个周期";

/**
 * 周期步进快捷键（issue #1904 / ADR-0141）：裸键 `,` 上一个周期 / `.` 下一个周期，
 * 语义等同点击 QuickTimeRange 的期间步进器 `<` / `>`（步进换算、数据期间边界钳制
 * 与 v-model 写回全在组件内单点，视图经 defineExpose 暴露的入口接线）。
 *
 * 断言全部对准用户可观察结果（列表过滤区间 / 重拉 / 页码 / 提示渲染）；删除视图中的
 * useTimePeriodShortcuts 接线调用、或删除 QuickTimeRange 的 defineExpose 步进入口，
 * 「, 步进」等用例即变红（接线证明，ADR-0087）。
 * 区间 ⇄ 期间换算与边界可达性数学见 time-period.test.ts；步进器按钮行为见
 * time-stepper.test.ts（断言不改）。今天是本文件的前提——fake timers 固定为
 * 2026-01-15（本地），步进落点与钳制边界随之确定。
 */
describe("TransactionsView 周期步进快捷键（issue #1904 / ADR-0141）", () => {
  // 月档边界 [2025-06, 2026-01]（最新端被「今天」抬升托在当前期间）
  const stepDb: Transaction[] = [
    makeTxn(1, "acc-1", { date: "2026-01-05" }),
    makeTxn(2, "acc-1", { date: "2025-12-10" }),
    makeTxn(3, "acc-1", { date: "2025-06-01" }),
  ];

  beforeEach(() => {
    setTxnDb([...stepDb]);
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 0, 15, 12, 0, 0));
    resetOverlays();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  function pressKey(key: string, init: KeyboardEventInit = {}) {
    window.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true, ...init }));
  }

  async function clickChip(wrapper: Awaited<ReturnType<typeof mountView>>, label: string) {
    const chip = wrapper.findAllComponents(NButton).find((b) => b.text().trim() === label)!;
    await chip.trigger("click");
    await flushPromises();
  }

  it(", 从当月步进上一个自然周期，列表按新期间重拉", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    const before = listCalls().length;
    pressKey(",");
    await flushPromises();
    expect(listCalls().length).toBe(before + 1);
    expect(lastListFilter()).toMatchObject({ page: 1, from: "2025-12-01", to: "2025-12-31" });
  });

  it(". 从上一周期走回当月", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    pressKey(",");
    await flushPromises();
    expect(lastListFilter()).toMatchObject({ from: "2025-12-01", to: "2025-12-31" });
    pressKey(".");
    await flushPromises();
    expect(lastListFilter()).toMatchObject({ page: 1, from: "2026-01-01", to: "2026-01-31" });
  });

  it("快捷键步进后翻页归零：第 2 页按 , 一步回第 1 页", async () => {
    // 混入一笔上月数据，使 ,（→ 2025-12）仍在数据边界内可达
    setTxnDb([
      makeTxn(99, "acc-1", { date: "2025-12-15" }),
      ...Array.from({ length: 25 }, (_, i) => makeTxn(i + 1, "acc-1", { date: "2026-01-05" })),
    ]);
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    tablePagination(wrapper).onChange(2);
    await flushPromises();
    expect(lastListFilter()).toMatchObject({ page: 2 });
    pressKey(",");
    await flushPromises();
    expect(lastListFilter()).toMatchObject({ page: 1, from: "2025-12-01", to: "2025-12-31" });
  });

  it("钳制：最新期间按 . 幂等无操作（页码与期间不动、不重拉）", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    const before = listCalls().length;
    pressKey(".");
    await flushPromises();
    expect(listCalls().length).toBe(before);
    expect(lastListFilter()).toMatchObject({ from: "2026-01-01", to: "2026-01-31" });
  });

  it("钳制：最早期间按 , 幂等无操作（年档游标 2025 早于最早交易期间）", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "去年");
    const before = listCalls().length;
    pressKey(",");
    await flushPromises();
    expect(listCalls().length).toBe(before);
    expect(lastListFilter()).toMatchObject({ from: "2025-01-01", to: "2025-12-31" });
  });

  it("「全部」无游标：两向按键均幂等无操作", async () => {
    await mountView();
    const before = listCalls().length;
    pressKey(",");
    pressKey(".");
    await flushPromises();
    expect(listCalls().length).toBe(before);
  });

  it("焦点在输入框时不触发", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    const input = document.createElement("input");
    document.body.appendChild(input);
    input.dispatchEvent(new KeyboardEvent("keydown", { key: ",", bubbles: true }));
    await flushPromises();
    expect(lastListFilter()).toMatchObject({ from: "2026-01-01", to: "2026-01-31" });
    input.remove();
  });

  it("弹层打开时不触发（Overlay Suppression）", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    const token = createOverlayToken("modal");
    token.set(true);
    pressKey(",");
    await flushPromises();
    expect(lastListFilter()).toMatchObject({ from: "2026-01-01", to: "2026-01-31" });
    token.set(false);
  });

  it("带修饰键不触发（含 < > 的 Shift 形态）", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    const before = listCalls().length;
    pressKey(",", { metaKey: true });
    pressKey(".", { ctrlKey: true });
    pressKey("<", { shiftKey: true });
    pressKey(">", { shiftKey: true });
    await flushPromises();
    expect(listCalls().length).toBe(before);
  });

  it("触控轴不绑监听（按键不步进）", async () => {
    const wrapper = await mountPhone();
    await clickChip(wrapper, "当月");
    const before = listCalls().length;
    pressKey(",");
    await flushPromises();
    expect(listCalls().length).toBe(before);
  });

  it("指针轴渲染键位提示（桌面档与移动档同源，提示贴步进器）", async () => {
    const desktop = await mountView();
    expect(desktop.text()).toContain(HINT_ZH);
    const mobile = await mountMobile();
    expect(mobile.text()).toContain(HINT_ZH);
  });

  it("触控轴不渲染提示", async () => {
    const wrapper = await mountPhone();
    expect(wrapper.text()).not.toContain(HINT_ZH);
  });
});

/** 键位匹配纯函数与输入轴无关，独立于视图挂载断言边界（修饰键闭集唯一来源）。 */
describe("matchPeriodStepShortcut（键位匹配纯函数）", () => {
  it("裸键 , / . 命中对应方向", () => {
    expect(matchPeriodStepShortcut(new KeyboardEvent("keydown", { key: "," }))).toBe("prev");
    expect(matchPeriodStepShortcut(new KeyboardEvent("keydown", { key: "." }))).toBe("next");
  });

  it("任何修饰键组合均不命中（Shift 形态的 < / > 亦不命中）", () => {
    for (const init of [
      { metaKey: true },
      { ctrlKey: true },
      { altKey: true },
      { shiftKey: true },
    ]) {
      expect(matchPeriodStepShortcut(new KeyboardEvent("keydown", { key: ",", ...init }))).toBe(
        null,
      );
      expect(matchPeriodStepShortcut(new KeyboardEvent("keydown", { key: ".", ...init }))).toBe(
        null,
      );
    }
    expect(
      matchPeriodStepShortcut(new KeyboardEvent("keydown", { key: "<", shiftKey: true })),
    ).toBe(null);
  });

  it("其他裸键不命中", () => {
    expect(matchPeriodStepShortcut(new KeyboardEvent("keydown", { key: "[" }))).toBe(null);
    expect(matchPeriodStepShortcut(new KeyboardEvent("keydown", { key: "a" }))).toBe(null);
  });
});

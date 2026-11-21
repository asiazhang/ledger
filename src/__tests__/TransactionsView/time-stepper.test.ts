import {
  setTxnDb,
  makeTxn,
  mountView,
  listCalls,
  lastListFilter,
  tablePagination,
  setReportDateRange,
} from "./common";
import { mockInvoke } from "@ledger/test-support/invoke-mock";
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { mount, flushPromises } from "@vue/test-utils";
import { NButton } from "naive-ui";
import { captureListenHandlers, type CapturedListener } from "@ledger/test-support/listen-mock";
import type { Transaction } from "@ledger/types";

describe("TransactionsView 期间步进器（issue #383 / #391）", () => {
  const stepDb: Transaction[] = [
    makeTxn(1, "acc-1", { date: "2026-01-05" }),
    makeTxn(2, "acc-1", { date: "2025-12-10" }),
    makeTxn(3, "acc-1", { date: "2025-06-01" }),
  ];

  let ledgerHandlers: CapturedListener[];

  beforeEach(() => {
    setTxnDb([...stepDb]);
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 0, 15, 12, 0, 0));
    ledgerHandlers = captureListenHandlers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  async function fireLedgerChanged() {
    ledgerHandlers.forEach((h) => h({ event: "ledger:changed", payload: null }));
    await flushPromises();
  }

  const rangeCalls = () => mockInvoke.mock.calls.filter(([cmd]) => cmd === "report_date_range");

  const stepButton = (wrapper: ReturnType<typeof mount>, key: "prev" | "next") =>
    wrapper
      .findAllComponents(NButton)
      .find((b) => b.attributes("aria-label") === (key === "prev" ? "上一个周期" : "下一个周期"))!;

  const periodLabel = (wrapper: ReturnType<typeof mount>) =>
    wrapper.find(".period-label-text").text();

  async function clickChip(wrapper: ReturnType<typeof mount>, label: string) {
    const chip = wrapper.findAllComponents(NButton).find((b) => b.text().trim() === label)!;
    await chip.trigger("click");
    await flushPromises();
  }

  async function step(wrapper: ReturnType<typeof mount>, key: "prev" | "next") {
    await stepButton(wrapper, key).trigger("click");
    await flushPromises();
  }

  const lit = (wrapper: ReturnType<typeof mount>, label: string) => {
    const chip = wrapper.findAllComponents(NButton).find((b) => b.text().trim() === label)!;
    return chip.props("type") === "primary";
  };

  it("「全部」默认态：步进置灰、标签为占位符，点击不触发请求", async () => {
    const wrapper = await mountView();
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(true);
    expect(stepButton(wrapper, "next").props("disabled")).toBe(true);
    expect(periodLabel(wrapper)).toBe("选择期间");
    const before = listCalls().length;
    await step(wrapper, "prev");
    await step(wrapper, "next");
    expect(listCalls().length).toBe(before);
  });

  it("点「当月」后 < 步进上月（跨年回退 2026-01 → 2025-12）：区间写回、标签更新、芯片全灭", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    expect(periodLabel(wrapper)).toBe("2026年1月");
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(false);
    const before = listCalls().length;
    await step(wrapper, "prev");
    expect(listCalls().length).toBe(before + 1);
    expect(lastListFilter()).toMatchObject({ page: 1, from: "2025-12-01", to: "2025-12-31" });
    expect(periodLabel(wrapper)).toBe("2025年12月");
    for (const label of ["全部", "当月", "当季", "当年", "去年"]) {
      expect(lit(wrapper, label)).toBe(false);
    }
  });

  it("界内空期间仍可达（2025-12 → 2025-11，空列表诚实显示）", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    await step(wrapper, "prev");
    await step(wrapper, "prev");
    expect(lastListFilter()).toMatchObject({ from: "2025-11-01", to: "2025-11-30" });
    expect(wrapper.text()).toContain("共 0 条");
  });

  it("> 原路走回：2025-12 → 2026-01 后「当月」芯片重新点亮", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    await step(wrapper, "prev");
    await step(wrapper, "next");
    expect(lastListFilter()).toMatchObject({ from: "2026-01-01", to: "2026-01-31" });
    expect(periodLabel(wrapper)).toBe("2026年1月");
    expect(lit(wrapper, "当月")).toBe(true);
  });

  it("钳制：> 在最新期间（2026-01）置灰，点击不触发请求", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    expect(stepButton(wrapper, "next").props("disabled")).toBe(true);
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(false);
    const before = listCalls().length;
    await step(wrapper, "next");
    expect(listCalls().length).toBe(before);
  });

  it("钳制：< 步进到最早期间（2025-06）后置灰，点击不触发请求", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    for (let i = 0; i < 7; i++) await step(wrapper, "prev");
    expect(periodLabel(wrapper)).toBe("2025年6月");
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(true);
    expect(stepButton(wrapper, "next").props("disabled")).toBe(false);
    const before = listCalls().length;
    await step(wrapper, "prev");
    expect(listCalls().length).toBe(before);
  });

  it("钳制随单位切换：当季（2026 一季度 = 最新）> 置灰、< 可步进", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "当季");
    expect(stepButton(wrapper, "next").props("disabled")).toBe(true);
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(false);
  });

  it("钳制随单位切换：去年（年游标 2025）< 到 2024 置灰、> 可走回 2026", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "去年");
    expect(periodLabel(wrapper)).toBe("2025年");
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(true);
    expect(stepButton(wrapper, "next").props("disabled")).toBe(false);
  });

  it("退化：边界拉取失败时不钳制，> 可步进进无数据期间（空列表诚实显示）", async () => {
    const failing: Promise<{ min_date: string | null; max_date: string | null }> = Promise.reject(
      new Error("boom"),
    );
    failing.catch(() => {});
    setReportDateRange(failing);
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    expect(stepButton(wrapper, "next").props("disabled")).toBe(false);
    await step(wrapper, "next");
    expect(lastListFilter()).toMatchObject({ page: 1, from: "2026-02-01", to: "2026-02-28" });
    expect(wrapper.text()).toContain("共 0 条");
  });

  it("退化：边界在途时不钳制（不阻塞步进），到达后恢复钳制", async () => {
    let resolveRange: (r: { min_date: string; max_date: string }) => void = () => {};
    setReportDateRange(
      new Promise((resolve) => {
        resolveRange = resolve;
      }),
    );
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    expect(stepButton(wrapper, "next").props("disabled")).toBe(false);
    await step(wrapper, "next");
    expect(lastListFilter()).toMatchObject({ from: "2026-02-01", to: "2026-02-28" });
    resolveRange({ min_date: "2025-06-01", max_date: "2026-01-05" });
    await flushPromises();
    expect(stepButton(wrapper, "next").props("disabled")).toBe(true);
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(false);
  });

  it("空库回退单当前期间：两向均不可步进", async () => {
    setTxnDb([]);
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(true);
    expect(stepButton(wrapper, "next").props("disabled")).toBe(true);
  });

  it("边界拉取：挂载拉取一次 + ledger:changed 失效重拉即时外扩", async () => {
    setTxnDb([makeTxn(1, "acc-1", { date: "2026-01-05" })]);
    const wrapper = await mountView();
    expect(rangeCalls().length).toBe(1);
    await clickChip(wrapper, "当月");
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(true);
    setTxnDb([
      makeTxn(1, "acc-1", { date: "2026-01-05" }),
      makeTxn(2, "acc-1", { date: "2025-08-01" }),
    ]);
    await fireLedgerChanged();
    expect(rangeCalls().length).toBe(2);
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(false);
  });

  it("边界拉取：ledger:changed 删除收窄即时跟随（箭头重新置灰）", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    await step(wrapper, "prev");
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(false);
    setTxnDb([makeTxn(1, "acc-1", { date: "2026-01-05" })]);
    await fireLedgerChanged();
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(true);
    expect(stepButton(wrapper, "next").props("disabled")).toBe(false);
  });

  it("季期间逐季步进（含跨年）：当季 < → 2025年四季度", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "当季");
    expect(periodLabel(wrapper)).toBe("2026年一季度");
    await step(wrapper, "prev");
    expect(lastListFilter()).toMatchObject({ from: "2025-10-01", to: "2025-12-31" });
    expect(periodLabel(wrapper)).toBe("2025年四季度");
    expect(wrapper.text()).toContain("共 1 条");
  });

  it("年期间逐年步进：当年 < → 2025 年，落点恰为「去年」预设 → 芯片点亮", async () => {
    const wrapper = await mountView();
    await clickChip(wrapper, "当年");
    expect(periodLabel(wrapper)).toBe("2026年");
    await step(wrapper, "prev");
    expect(lastListFilter()).toMatchObject({ from: "2025-01-01", to: "2025-12-31" });
    expect(periodLabel(wrapper)).toBe("2025年");
    expect(lit(wrapper, "去年")).toBe(true);
    expect(lit(wrapper, "当年")).toBe(false);
  });

  it("步进后翻页归零：第 2 页 < 一步回第 1 页", async () => {
    setTxnDb([
      makeTxn(99, "acc-1", { date: "2025-12-15" }),
      ...Array.from({ length: 25 }, (_, i) => makeTxn(i + 1, "acc-1", { date: "2026-01-05" })),
    ]);
    const wrapper = await mountView();
    await clickChip(wrapper, "当月");
    tablePagination(wrapper).onChange(2);
    await flushPromises();
    expect(lastListFilter()).toMatchObject({ page: 2 });
    await step(wrapper, "prev");
    expect(lastListFilter()).toMatchObject({ page: 1, from: "2025-12-01", to: "2025-12-31" });
  });
});

import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { captureListenHandlers } from "@ledger/test-support/listen-mock";
import { setFakeMedia } from "@ledger/test-support/media-mock";
import { mount, flushPromises, type VueWrapper } from "@vue/test-utils";
import { NButton, NDatePicker } from "naive-ui";
import { resetOverlays, hasOpenOverlay, openOverlayNames } from "@ledger/ui-kit/overlayRegistry";
import { registerToastSink } from "@ledger/loadable";
import { makeFakeSink, resetToastSink } from "./factories";
import AppDatePicker from "@ledger/ui-kit/AppDatePicker.vue";
import QuickTimeRange from "@/components/QuickTimeRange.vue";
import { DATED_TIME_PERIOD_PRESETS, type NullableDateRange } from "@ledger/utils/time-period";

beforeAll(() => {
  Element.prototype.scrollTo = () => {};
});

describe("QuickTimeRange 共享受控组件（issue #410）", () => {
  const BOUNDARY = { min_date: "2025-06-01", max_date: "2026-01-05" };

  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date(2026, 0, 15, 12, 0, 0));
    resetToastSink();
    resetOverlays();
    wireInvokeSeam({
      overrides: {
        report_date_range: BOUNDARY,
      },
    });
  });

  afterEach(() => {
    vi.useRealTimers();
    resetToastSink();
    resetOverlays();
  });

  function mountRange(modelValue: NullableDateRange = { from: null, to: null }) {
    return mount(QuickTimeRange, { props: { modelValue } });
  }

  const chip = (wrapper: VueWrapper, label: string) =>
    wrapper.findAllComponents(NButton).find((b) => b.text().trim() === label)!;

  async function clickChip(wrapper: VueWrapper, label: string) {
    await chip(wrapper, label).trigger("click");
    await flushPromises();
  }

  const lit = (wrapper: VueWrapper, label: string) =>
    chip(wrapper, label).props("type") === "primary";

  const lastEmitted = (wrapper: VueWrapper): NullableDateRange | undefined => {
    const events = wrapper.emitted("update:modelValue") as Array<[NullableDateRange]> | undefined;
    return events?.[events.length - 1]?.[0];
  };

  const emitCount = (wrapper: VueWrapper) => wrapper.emitted("update:modelValue")?.length ?? 0;

  const stepButton = (wrapper: VueWrapper, key: "prev" | "next") =>
    wrapper
      .findAllComponents(NButton)
      .find((b) => b.attributes("aria-label") === (key === "prev" ? "上一个周期" : "下一个周期"))!;

  const periodLabel = (wrapper: VueWrapper) => wrapper.find(".period-label-text").text();

  async function stepAndConsume(wrapper: VueWrapper, key: "prev" | "next") {
    await stepButton(wrapper, key).trigger("click");
    await flushPromises();
    const next = lastEmitted(wrapper)!;
    await wrapper.setProps({ modelValue: next });
    return next;
  }

  it("默认态（双空区间）：五枚芯片、「全部」点亮、步进双向置灰、标签占位、挂载拉取边界一次", async () => {
    const wrapper = mountRange();
    await flushPromises();
    for (const label of ["全部", "当月", "当季", "当年", "去年"]) {
      expect(chip(wrapper, label).exists()).toBe(true);
    }
    expect(lit(wrapper, "全部")).toBe(true);
    for (const label of ["当月", "当季", "当年", "去年"]) {
      expect(lit(wrapper, label)).toBe(false);
    }
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(true);
    expect(stepButton(wrapper, "next").props("disabled")).toBe(true);
    expect(periodLabel(wrapper)).toBe("选择期间");
    expect(mockInvoke.mock.calls.filter(([cmd]) => cmd === "report_date_range")).toHaveLength(1);
  });

  it("点日期芯片 emit 精确自然周期快照（当月/当季/当年/去年各按定义换算）", async () => {
    const wrapper = mountRange();
    await flushPromises();
    await clickChip(wrapper, "当月");
    expect(lastEmitted(wrapper)).toEqual({ from: "2026-01-01", to: "2026-01-31" });
    await clickChip(wrapper, "当季");
    expect(lastEmitted(wrapper)).toEqual({ from: "2026-01-01", to: "2026-03-31" });
    await clickChip(wrapper, "当年");
    expect(lastEmitted(wrapper)).toEqual({ from: "2026-01-01", to: "2026-12-31" });
    await clickChip(wrapper, "去年");
    expect(lastEmitted(wrapper)).toEqual({ from: "2025-01-01", to: "2025-12-31" });
  });

  it("点「全部」emit 双空区间（无日期过滤 = 默认态）", async () => {
    const wrapper = mountRange({ from: "2026-01-01", to: "2026-01-31" });
    await flushPromises();
    await clickChip(wrapper, "全部");
    expect(lastEmitted(wrapper)).toEqual({ from: null, to: null });
  });

  it("受控高亮：点亮纯由 prop 区间派生，组件不自持选择状态", async () => {
    const wrapper = mountRange({ from: "2025-01-01", to: "2025-12-31" });
    await flushPromises();
    expect(lit(wrapper, "去年")).toBe(true);
    expect(lit(wrapper, "全部")).toBe(false);
    const historical = mountRange({ from: "2025-12-01", to: "2025-12-31" });
    await flushPromises();
    for (const label of ["全部", "当月", "当季", "当年", "去年"]) {
      expect(lit(historical, label)).toBe(false);
    }
  });

  it("步进换算：< 从当月落上月（跨年回退 2026-01 → 2025-12），emit 上月快照、标签跟随", async () => {
    const wrapper = mountRange({ from: "2026-01-01", to: "2026-01-31" });
    await flushPromises();
    expect(periodLabel(wrapper)).toBe("2026年1月");
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(false);
    const range = await stepAndConsume(wrapper, "prev");
    expect(range).toEqual({ from: "2025-12-01", to: "2025-12-31" });
    expect(periodLabel(wrapper)).toBe("2025年12月");
  });

  it("边界外步进置灰：最新期间 > 置灰；走到最早期间（2025-06）后 < 置灰、> 可走回；置灰点击不 emit", async () => {
    const wrapper = mountRange({ from: "2026-01-01", to: "2026-01-31" });
    await flushPromises();
    expect(stepButton(wrapper, "next").props("disabled")).toBe(true);
    let range: NullableDateRange = { from: "2026-01-01", to: "2026-01-31" };
    for (let i = 0; i < 7; i++) range = await stepAndConsume(wrapper, "prev");
    expect(range).toEqual({ from: "2025-06-01", to: "2025-06-30" });
    expect(periodLabel(wrapper)).toBe("2025年6月");
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(true);
    expect(stepButton(wrapper, "next").props("disabled")).toBe(false);
    const before = emitCount(wrapper);
    await stepButton(wrapper, "prev").trigger("click");
    await flushPromises();
    expect(emitCount(wrapper)).toBe(before);
    await stepAndConsume(wrapper, "next");
    expect(lastEmitted(wrapper)).toEqual({ from: "2025-07-01", to: "2025-07-31" });
  });

  it("退化：边界拉取失败时不钳制（> 可步进）、不阻塞快捷选择、静默不 toast（silent 实例）", async () => {
    const sink = makeFakeSink();
    registerToastSink(sink);
    const failing: Promise<{ min_date: string | null; max_date: string | null }> = Promise.reject(
      new Error("boom"),
    );
    failing.catch(() => {});
    wireInvokeSeam({
      overrides: {
        report_date_range: () => failing,
      },
    });
    const wrapper = mountRange({ from: "2026-01-01", to: "2026-01-31" });
    await flushPromises();
    expect(stepButton(wrapper, "next").props("disabled")).toBe(false);
    expect(sink.error).not.toHaveBeenCalled();
  });

  it("面板：type 随当前游标单位切换（月/季/年），边界外月份置灰", async () => {
    const wrapper = mountRange({ from: "2026-01-01", to: "2026-01-31" });
    await flushPromises();
    const picker = wrapper.findComponent(NDatePicker);
    expect(wrapper.findComponent(AppDatePicker).exists()).toBe(true);
    expect(picker.props("type")).toBe("month");
    const isDisabled = picker.props("isDateDisabled") as (
      timestamp: number,
      detail: unknown,
    ) => boolean;
    expect(isDisabled(0, { type: "month", year: 2025, month: 4 })).toBe(true);
    expect(isDisabled(0, { type: "month", year: 2025, month: 5 })).toBe(false);
  });

  it("面板选择：点选边界内期间 emit 精确快照并关闭面板（弹层注册表同步撤销）", async () => {
    const wrapper = mountRange({ from: "2026-01-01", to: "2026-01-31" });
    await flushPromises();
    await wrapper.find(".period-label").trigger("click");
    await flushPromises();
    expect(hasOpenOverlay()).toBe(true);
    const picker = wrapper.findComponent(NDatePicker);
    picker.vm.$emit("update:value", new Date(2025, 11, 1).getTime());
    await flushPromises();
    expect(lastEmitted(wrapper)).toEqual({ from: "2025-12-01", to: "2025-12-31" });
    expect(hasOpenOverlay()).toBe(false);
  });

  it("键盘可达：期间标签聚焦后 Enter/Space 打开面板，aria-expanded 随开合（issue #425）", async () => {
    const wrapper = mountRange({ from: "2026-01-01", to: "2026-01-31" });
    await flushPromises();
    const trigger = wrapper.find('[aria-haspopup="dialog"]');
    expect(trigger.exists()).toBe(true);
    expect(trigger.attributes("aria-expanded")).toBe("false");
    await trigger.trigger("keydown", { key: "Enter" });
    await flushPromises();
    expect(hasOpenOverlay()).toBe(true);
    expect(trigger.attributes("aria-expanded")).toBe("true");
    wrapper.findComponent(NDatePicker).vm.$emit("update:show", false);
    await flushPromises();
    expect(trigger.attributes("aria-expanded")).toBe("false");
    await trigger.trigger("keydown", { key: " " });
    await flushPromises();
    expect(hasOpenOverlay()).toBe(true);
  });

  it("面板开/关上报弹层注册表（Overlay Suppression 不回退）", async () => {
    const wrapper = mountRange();
    await flushPromises();
    expect(hasOpenOverlay()).toBe(false);
    await wrapper.find(".period-label").trigger("click");
    await flushPromises();
    expect(openOverlayNames()).toContain("date-picker");
    wrapper.findComponent(NDatePicker).vm.$emit("update:show", false);
    await flushPromises();
    expect(hasOpenOverlay()).toBe(false);
  });

  it("触控轴触控目标基线（issue #849 / ADR-0088 决策 6）：芯片、步进器与期间标签按钮本体 ≥48px 高，指针轴不挂零变化", async () => {
    setFakeMedia({ width: 1280, hover: "none", pointer: "coarse" });
    const wrapper = mountRange();
    await flushPromises();
    for (const label of ["全部", "当月", "当季", "当年", "去年"]) {
      expect(chip(wrapper, label).attributes("style")).toContain("min-height: 48px");
    }
    expect(stepButton(wrapper, "prev").attributes("style")).toContain("min-height: 48px");
    expect(stepButton(wrapper, "next").attributes("style")).toContain("min-height: 48px");
    const labelButton = wrapper
      .findAllComponents(NButton)
      .find((b) => b.find(".period-label-text").exists())!;
    expect(labelButton.attributes("style")).toContain("min-height: 48px");

    setFakeMedia({ width: 1280, hover: "hover", pointer: "fine" });
    const desktop = mountRange();
    await flushPromises();
    expect(chip(desktop, "全部").attributes("style") ?? "").not.toContain("min-height");
    expect(stepButton(desktop, "prev").attributes("style") ?? "").not.toContain("min-height");
  });

  it("触控轴交互语义不变：点芯片仍 emit 快照、步进仍换算（触控目标只是呈现层，不触语义）", async () => {
    setFakeMedia({ width: 1280, hover: "none", pointer: "coarse" });
    const wrapper = mountRange();
    await flushPromises();
    await clickChip(wrapper, "当月");
    expect(lastEmitted(wrapper)).toEqual({ from: "2026-01-01", to: "2026-01-31" });
  });

  it("presets prop 收窄芯片闭集（报表页日期闭集消费形态，ADR-0057）：无「全部」", async () => {
    const wrapper = mount(QuickTimeRange, {
      props: {
        modelValue: { from: "2026-01-01", to: "2026-12-31" } as NullableDateRange,
        presets: DATED_TIME_PERIOD_PRESETS,
      },
    });
    await flushPromises();
    expect(chip(wrapper, "全部")).toBeUndefined();
    for (const label of ["当月", "当季", "当年", "去年"]) {
      expect(chip(wrapper, label).exists()).toBe(true);
    }
  });

  it("数据期间边界失效重拉：ledger:changed 后即时外扩（钳制边界跟随新数据）", async () => {
    const handlers = captureListenHandlers();
    wireInvokeSeam({
      overrides: {
        report_date_range: { min_date: "2026-01-05", max_date: "2026-01-05" },
      },
    });
    const wrapper = mountRange({ from: "2026-01-01", to: "2026-01-31" });
    await flushPromises();
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(true);
    wireInvokeSeam({
      overrides: {
        report_date_range: { min_date: "2025-08-01", max_date: "2026-01-05" },
      },
    });
    handlers.forEach((h) => h({ event: "ledger:changed", payload: null }));
    await flushPromises();
    expect(stepButton(wrapper, "prev").props("disabled")).toBe(false);
  });

  it("defineExpose 步进入口：stepPrev/stepNext 走同一 onStepPeriod 出口（issue #1904）", async () => {
    const wrapper = mountRange({ from: "2026-01-01", to: "2026-01-31" });
    await flushPromises();
    const vm = wrapper.vm as unknown as { stepPrev: () => void; stepNext: () => void };
    vm.stepPrev();
    await flushPromises();
    expect(lastEmitted(wrapper)).toEqual({ from: "2025-12-01", to: "2025-12-31" });
    await wrapper.setProps({ modelValue: { from: "2025-12-01", to: "2025-12-31" } });
    vm.stepNext();
    await flushPromises();
    expect(lastEmitted(wrapper)).toEqual({ from: "2026-01-01", to: "2026-01-31" });
    await wrapper.setProps({ modelValue: { from: "2026-01-01", to: "2026-01-31" } });
    const before = emitCount(wrapper);
    vm.stepNext();
    await flushPromises();
    expect(emitCount(wrapper)).toBe(before);
  });

  it("stepShortcutKeys：缺省不渲染键位标注（其他消费页零变化）；传入后指针轴按钮内嵌键位、触控轴不渲染", async () => {
    setFakeMedia({ width: 1280, hover: "hover", pointer: "fine" });
    const plain = mountRange({ from: "2026-01-01", to: "2026-01-31" });
    await flushPromises();
    expect(plain.find(".period-key").exists()).toBe(false);

    const keys = { prev: ",", next: "." };
    const inline = mount(QuickTimeRange, {
      props: { modelValue: { from: "2026-01-01", to: "2026-01-31" }, stepShortcutKeys: keys },
    });
    await flushPromises();
    const keySpans = inline.findAll(".period-key");
    expect(keySpans.map((s) => s.text())).toEqual([",", "."]);
    expect(inline.text()).not.toContain("上一个周期");
    expect(inline.find('[aria-label="上一个周期"]').attributes("title")).toBe("上一个周期 (,)");

    setFakeMedia({ width: 1280, hover: "none", pointer: "coarse" });
    const touch = mount(QuickTimeRange, {
      props: { modelValue: { from: "2026-01-01", to: "2026-01-31" }, stepShortcutKeys: keys },
    });
    await flushPromises();
    expect(touch.find(".period-key").exists()).toBe(false);
  });
});

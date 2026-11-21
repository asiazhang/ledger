import { describe, it, expect, beforeEach } from "vitest";
import { mount } from "@vue/test-utils";
import {
  captureListenHandlers,
  mockListen,
  type CapturedListener,
} from "@ledger/test-support/listen-mock";
import HistoryBackfillIndicator from "@/investment/HistoryBackfillIndicator.vue";
import {
  HISTORY_BACKFILL_PROGRESS_EVENT,
  resetHistoryBackfillForTest,
} from "@/investment/useHistoryBackfill";

function fire(handlers: CapturedListener[], payload: unknown): void {
  for (const handler of handlers) handler({ event: HISTORY_BACKFILL_PROGRESS_EVENT, payload });
}

describe("HistoryBackfillIndicator 后台补全静默计数（投资页）", () => {
  let handlers: CapturedListener[];

  beforeEach(() => {
    mockListen.mockReset();
    resetHistoryBackfillForTest();
    handlers = captureListenHandlers();
  });

  it("无在途轮次不渲染", () => {
    const wrapper = mount(HistoryBackfillIndicator);
    expect(wrapper.find('[data-testid="history-backfill-indicator"]').exists()).toBe(false);
  });

  it("在途轮次渲染只读计数（存在性断言：删掉渲染即变红）", async () => {
    const wrapper = mount(HistoryBackfillIndicator);
    fire(handlers, { done: 137, total: 228 });
    await wrapper.vm.$nextTick();
    const indicator = wrapper.find('[data-testid="history-backfill-indicator"]');
    expect(indicator.exists()).toBe(true);
    expect(indicator.text()).toContain("历史数据补全中 137/228");
    expect(indicator.find('[role="progressbar"]').exists()).toBe(false);
    expect(indicator.find("button").exists()).toBe(false);
  });
  it("页级明细已随换源退役：仅渲染标的级计数行", async () => {
    const wrapper = mount(HistoryBackfillIndicator);
    fire(handlers, { done: 3, total: 228 });
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="history-backfill-indicator-fund"]').exists()).toBe(false);
  });

  it("终态静默收起：done ≥ total 后组件消失", async () => {
    const wrapper = mount(HistoryBackfillIndicator);
    fire(handlers, { done: 1, total: 228 });
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="history-backfill-indicator"]').exists()).toBe(true);
    fire(handlers, { done: 228, total: 228 });
    await wrapper.vm.$nextTick();
    expect(wrapper.find('[data-testid="history-backfill-indicator"]').exists()).toBe(false);
  });
});

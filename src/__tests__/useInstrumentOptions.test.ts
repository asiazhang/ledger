import { describe, it, expect, vi, beforeEach } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { withSetup } from "@ledger/test-support/mount";
import { SEARCH_DEBOUNCE_MS } from "@/composables/search-debounce";
import { useInstrumentOptions } from "@/investment/useInstrumentOptions";
import { makeInstrument, resetToastSink } from "./factories";

/**
 * 标的下拉候选面域件的领域断言单点（issue #1798）：候选投影（「代码 · 名称」label
 * 拼法、无名称退化裸代码）与钉住合并（防丢失、去重、合入端 head/tail、清除）钉在
 * 本文件；取数编排机制断言（防抖、在途竞态、失败吞错）归 useInstrumentSearch 模块
 * 单测不在此重复。消费方（盈亏筛选 useRealizedPnl / 投资表单 useInvestmentForm）
 * 只保留各自领域断言（筛选刷新、基金判定、select 回填）。
 */

beforeEach(() => {
  // 每用例复位为 no-op，模拟「注册前」默认态，防模块级 sink 状态串扰
  resetToastSink();
});

/** 走一次搜索到结果落位（防抖时长单源 SEARCH_DEBOUNCE_MS） */
async function searchAndSettle(
  shell: ReturnType<typeof useInstrumentOptions>,
  query: string,
): Promise<void> {
  shell.search(query);
  await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS);
  await flushPromises();
}

describe("useInstrumentOptions 标的下拉候选面（issue #1798）", () => {
  it("候选投影：「代码 · 名称」label、id 为 value", async () => {
    vi.useFakeTimers();
    try {
      wireInvokeSeam({
        defaults: {
          list_instruments: {
            items: [makeInstrument({ id: "inst-1", symbol: "600000", name: "浦发银行" })],
            total: 1,
          },
        },
      });
      const shell = withSetup(() => useInstrumentOptions("tail"));
      await searchAndSettle(shell, "浦发");
      expect(shell.options.value).toEqual([{ label: "600000 · 浦发银行", value: "inst-1" }]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("无名称（含空串）退化为裸代码 label", async () => {
    vi.useFakeTimers();
    try {
      wireInvokeSeam({
        defaults: {
          list_instruments: {
            items: [makeInstrument({ id: "inst-1", symbol: "600000", name: null })],
            total: 1,
          },
        },
      });
      const shell = withSetup(() => useInstrumentOptions("tail"));
      await searchAndSettle(shell, "浦发");
      expect(shell.options.value).toEqual([{ label: "600000", value: "inst-1" }]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("tail 钉住：钉住标的不在新候选时投影合入尾部；pin(null) 清除", async () => {
    vi.useFakeTimers();
    try {
      wireInvokeSeam({
        defaults: {
          list_instruments: {
            items: [makeInstrument({ id: "inst-1", symbol: "600000", name: "浦发银行" })],
            total: 1,
          },
        },
      });
      const shell = withSetup(() => useInstrumentOptions("tail"));
      await searchAndSettle(shell, "浦发");
      // 钉住不在候选面的标的：合入尾部，label 走同一投影单点
      shell.pin(makeInstrument({ id: "inst-9", symbol: "APPL", name: "苹果" }));
      expect(shell.options.value).toEqual([
        { label: "600000 · 浦发银行", value: "inst-1" },
        { label: "APPL · 苹果", value: "inst-9" },
      ]);
      // 清除钉住：候选面回到纯搜索结果
      shell.pin(null);
      expect(shell.options.value).toEqual([{ label: "600000 · 浦发银行", value: "inst-1" }]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("head 钉住：钉住标的合入头部（表单编辑回填形态：回填项优先可见）", async () => {
    vi.useFakeTimers();
    try {
      wireInvokeSeam({
        defaults: {
          list_instruments: {
            items: [makeInstrument({ id: "inst-1", symbol: "600000", name: "浦发银行" })],
            total: 1,
          },
        },
      });
      const shell = withSetup(() => useInstrumentOptions("head"));
      await searchAndSettle(shell, "浦发");
      shell.pin(makeInstrument({ id: "inst-9", symbol: "APPL", name: "苹果" }));
      expect(shell.options.value).toEqual([
        { label: "APPL · 苹果", value: "inst-9" },
        { label: "600000 · 浦发银行", value: "inst-1" },
      ]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("钉住标的已含于搜索结果时不重复（去重）", async () => {
    vi.useFakeTimers();
    try {
      wireInvokeSeam({
        defaults: {
          list_instruments: {
            items: [makeInstrument({ id: "inst-1", symbol: "600000", name: "浦发银行" })],
            total: 1,
          },
        },
      });
      const shell = withSetup(() => useInstrumentOptions("tail"));
      shell.pin(makeInstrument({ id: "inst-1", symbol: "600000", name: "浦发银行" }));
      await searchAndSettle(shell, "浦发");
      expect(shell.options.value).toEqual([{ label: "600000 · 浦发银行", value: "inst-1" }]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("findInstrument：按 id 查原始候选（基金判定等领域形态归消费方），未命中 undefined", async () => {
    vi.useFakeTimers();
    try {
      wireInvokeSeam({
        defaults: {
          list_instruments: {
            items: [makeInstrument({ id: "inst-1", type: "fund", name: "某基金" })],
            total: 1,
          },
        },
      });
      const shell = withSetup(() => useInstrumentOptions("tail"));
      expect(shell.findInstrument("inst-1")).toBeUndefined();
      await searchAndSettle(shell, "浦发");
      expect(shell.findInstrument("inst-1")?.type).toBe("fund");
      expect(shell.findInstrument("missing")).toBeUndefined();
    } finally {
      vi.useRealTimers();
    }
  });
});

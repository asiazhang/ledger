import { describe, it, expect, vi } from "vitest";
import { flushPromises } from "@vue/test-utils";
import { deferred } from "@ledger/test-support/deferred";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { withSetup } from "@ledger/test-support/mount";
import { registerToastSink } from "@ledger/loadable";
import { SEARCH_DEBOUNCE_MS } from "@/composables/search-debounce";
import { useInstrumentSearch } from "@/investment/useInstrumentSearch";
import { makeFakeSink, makeInstrument, resetToastSink } from "./factories";
import type { Instrument } from "@ledger/types";

function resultFor(symbol: string, name: string): { items: Instrument[]; total: number } {
  return { items: [makeInstrument({ id: `ins-${symbol}`, symbol, name })], total: 1 };
}

function listInstrumentCalls(): unknown[][] {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === "list_instruments");
}

beforeEach(() => {
  resetToastSink();
});

describe("useInstrumentSearch 标的远程搜索编排（issue #1308）", () => {
  it("防抖：连续输入只发最后一次查询，候选随结果落位", async () => {
    vi.useFakeTimers();
    try {
      wireInvokeSeam({ defaults: { list_instruments: resultFor("浦发银行", "后查") } });
      const { search, items, searching } = withSetup(() => useInstrumentSearch());
      search("浦发");
      search("浦发银");
      search("浦发银行");
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS);
      await flushPromises();

      const calls = listInstrumentCalls();
      expect(calls.length).toBe(1);
      expect(items.value.map((i) => i.symbol)).toEqual(["浦发银行"]);
      expect(searching.value).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("请求形状：{ search, page_size: 50 }，查询词 trim 后携带", async () => {
    vi.useFakeTimers();
    try {
      wireInvokeSeam({ defaults: { list_instruments: resultFor("NVDA", "英伟达") } });
      const { search, items } = withSetup(() => useInstrumentSearch());
      search("  NVDA  ");
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS);
      await flushPromises();

      const calls = listInstrumentCalls();
      expect(calls.length).toBe(1);
      expect(calls[0]![1]).toEqual({ filter: { search: "NVDA", page_size: 50 } });
      expect(items.value.map((i) => i.symbol)).toEqual(["NVDA"]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("空查询（trim 后为空）不发请求：候选清空、searching 置收", async () => {
    vi.useFakeTimers();
    try {
      wireInvokeSeam({ defaults: { list_instruments: resultFor("NVDA", "英伟达") } });
      const { search, items, searching } = withSetup(() => useInstrumentSearch());
      search("NVDA");
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS);
      await flushPromises();
      expect(items.value.length).toBe(1);

      const callsBefore = listInstrumentCalls().length;
      search("   ");
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS);
      await flushPromises();

      expect(listInstrumentCalls().length).toBe(callsBefore);
      expect(items.value).toEqual([]);
      expect(searching.value).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("失败刻意吞错：静默清空候选、searching 置收，无 toast 不置 error 通道", async () => {
    vi.useFakeTimers();
    try {
      const sink = makeFakeSink();
      registerToastSink(sink);
      wireInvokeSeam({
        overrides: { list_instruments: () => Promise.reject(new Error("搜索失败")) },
      });
      const { search, items, searching } = withSetup(() => useInstrumentSearch());
      search("浦发");
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS);
      await flushPromises();

      expect(items.value).toEqual([]);
      expect(searching.value).toBe(false);
      expect(sink.error).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("useInstrumentSearch 在途竞态（issue #1401 编排内化）", () => {
  it("先发请求迟到不覆盖后发结果：候选呈现后发查询的标的", async () => {
    vi.useFakeTimers();
    try {
      const first = deferred<{ items: Instrument[]; total: number }>();
      const second = deferred<{ items: Instrument[]; total: number }>();
      let calls = 0;
      wireInvokeSeam({
        overrides: {
          list_instruments: () => {
            calls += 1;
            return calls === 1 ? first.promise : second.promise;
          },
        },
      });
      const { search, items, searching } = withSetup(() => useInstrumentSearch());
      search("AAA");
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS);
      expect(searching.value).toBe(true);
      search("BBB");
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS);
      expect(calls).toBe(2);

      second.resolve(resultFor("BBB", "后发"));
      await flushPromises();
      first.resolve(resultFor("AAA", "先发"));
      await flushPromises();

      expect(items.value.map((i) => i.symbol)).toEqual(["BBB"]);
      expect(searching.value).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it("清空输入作废在途：先前发出的非空请求迟到不再填回候选", async () => {
    vi.useFakeTimers();
    try {
      const pending = deferred<{ items: Instrument[]; total: number }>();
      let calls = 0;
      wireInvokeSeam({
        overrides: {
          list_instruments: () => {
            calls += 1;
            return pending.promise;
          },
        },
      });
      const { search, items, searching } = withSetup(() => useInstrumentSearch());
      search("AAA");
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS);
      expect(calls).toBe(1);
      search("");
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS);
      expect(items.value).toEqual([]);

      pending.resolve(resultFor("AAA", "先发"));
      await flushPromises();
      expect(items.value).toEqual([]);
      expect(searching.value).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });
});

import { describe, it, expect, vi, beforeEach } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { flushPromises, mount } from "@vue/test-utils";
import { withSetup } from "@ledger/test-support/mount";
import { defineComponent } from "vue";
import { SEARCH_DEBOUNCE_MS } from "@/composables/search-debounce";
import { useReferenceStore } from "@/stores/reference";
import { useRealizedPnl } from "@/investment/useRealizedPnl";
import { useInvestmentsSessionStore } from "@/investment/investments-session";
import { registerToastSink } from "@ledger/loadable";
import type { RealizedPnlSummary } from "@ledger/types";
import {
  makeAccount,
  makeFakeSink,
  makeInstrument,
  makePnlSummary,
  mockAccounts,
  resetToastSink,
} from "./factories";

const mockSummary = makePnlSummary();

const BASE_DEFAULTS = {
  realized_pnl_summary: mockSummary,
  list_instruments: { items: [makeInstrument({ id: "inst-1" })], total: 1 },
};

const REFERENCE_OVERRIDES = { list_accounts: mockAccounts };

const Host = defineComponent({
  setup() {
    return { shell: useRealizedPnl() };
  },
  template: "<div />",
});

beforeEach(async () => {
  wireInvokeSeam({ defaults: BASE_DEFAULTS, overrides: REFERENCE_OVERRIDES });
  resetToastSink();
  const store = useReferenceStore();
  await store.refresh();
});

describe("useRealizedPnl 已实现盈亏数据层", () => {
  it("加载已实现盈亏汇总（盈亏页两表的同一数据源）", async () => {
    const { summary, loading, refresh } = withSetup(() => useRealizedPnl());
    expect(summary.value).toBeNull();
    await refresh();
    expect(loading.value).toBe(false);
    expect(summary.value).toEqual(mockSummary);
  });

  it("无筛选时不带 filter 参数（后端全表口径）", async () => {
    const { refresh } = withSetup(() => useRealizedPnl());
    await refresh();
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "realized_pnl_summary");
    expect(call![1]).toEqual({ filter: null });
  });

  it("筛选变化翻页归零（issue #1795）：账户/标的筛选实际变化即回第一页", () => {
    const { selectedAccountId, selectedInstrumentId } = withSetup(() => useRealizedPnl());
    const session = useInvestmentsSessionStore();
    session.setPnlYearPage(3);
    session.setPnlAccountPage(2);
    selectedAccountId.value = "acc-1";
    expect(session.pnlYearPage).toBe(1);
    expect(session.pnlAccountPage).toBe(1);
    session.setPnlYearPage(2);
    selectedAccountId.value = "acc-1";
    expect(session.pnlYearPage).toBe(2);
    selectedInstrumentId.value = "inst-1";
    expect(session.pnlYearPage).toBe(1);
  });

  it("竞态：后发覆盖先发，迟到前发结果不覆写 summary 终态", async () => {
    let releaseFirst!: (summary: RealizedPnlSummary) => void;
    let calls = 0;
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...REFERENCE_OVERRIDES,
        realized_pnl_summary: () => {
          calls += 1;
          if (calls === 1) {
            return new Promise<RealizedPnlSummary>((resolve) => {
              releaseFirst = resolve;
            });
          }
          return Promise.resolve(
            makePnlSummary({
              by_year: [
                {
                  year: "2026",
                  native_currency: "CNY",
                  realized_pnl_cents: 777,
                  dividend_cents: 0,
                  realized_gain_cents: 777,
                },
              ],
            }),
          );
        },
      },
    });
    const { summary, error, refresh } = withSetup(() => useRealizedPnl());
    const first = refresh();
    const second = refresh();
    await second;
    expect(summary.value!.by_year[0].realized_pnl_cents).toBe(777);

    releaseFirst(mockSummary);
    await first;
    expect(summary.value!.by_year[0].realized_pnl_cents).toBe(777);
    expect(error.value).toBeNull();
  });

  it("账户/标的筛选生效：refresh 闭包自读当前筛选（0 元闭包，发起时点即最新值）", async () => {
    const { selectedAccountId, selectedInstrumentId, refresh } = withSetup(() => useRealizedPnl());
    await refresh();

    selectedAccountId.value = "acc-1";
    await refresh();
    const accountCall = mockInvoke.mock.calls.filter(([cmd]) => cmd === "realized_pnl_summary")[2];
    expect(accountCall![1]).toEqual({ filter: { account_id: "acc-1" } });

    selectedInstrumentId.value = "inst-1";
    await refresh();
    const bothCall = mockInvoke.mock.calls.filter(([cmd]) => cmd === "realized_pnl_summary")[3];
    expect(bothCall![1]).toEqual({ filter: { account_id: "acc-1", instrument_id: "inst-1" } });
  });

  it("onSelectInstrument 更新标的筛选并立即刷新", async () => {
    const { selectedInstrumentId, onSelectInstrument } = withSetup(() => useRealizedPnl());
    onSelectInstrument("inst-1");
    await flushPromises();
    expect(selectedInstrumentId.value).toBe("inst-1");
    const calls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "realized_pnl_summary");
    expect(calls.length).toBe(2);
    expect(calls.at(-1)![1]).toEqual({ filter: { instrument_id: "inst-1" } });
  });

  it("账户选项只含投资账户：谓词收口参考 store（隐藏投资账户保留、非投资类型排除）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        list_accounts: [
          ...mockAccounts,
          makeAccount({ id: "acc-cash", name: "现金钱包", type: "cash" }),
          makeAccount({ id: "acc-hidden", name: "隐藏证券户", is_hidden: true }),
        ],
      },
    });
    await useReferenceStore().refresh();
    const { accountOptions } = withSetup(() => useRealizedPnl());
    expect(accountOptions.value).toEqual([
      { label: "证券账户A", value: "acc-1" },
      { label: "隐藏证券户", value: "acc-hidden" },
    ]);
  });
});

describe("useRealizedPnl 账户归档（spec #1952 / ADR-0142 决策 4）", () => {
  it("归档投资账户不进新选择；已选筛选值恰是归档账户时补回并标「已归档」", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        list_accounts: [
          ...mockAccounts,
          makeAccount({ id: "acc-arch", name: "旧证券户", is_archived: true }),
        ],
      },
    });
    await useReferenceStore().refresh();
    const { accountOptions, selectedAccountId } = withSetup(() => useRealizedPnl());
    expect(accountOptions.value.map((o) => o.value)).toEqual(["acc-1"]);
    selectedAccountId.value = "acc-arch";
    expect(accountOptions.value).toEqual([
      { label: "旧证券户（已归档）", value: "acc-arch" },
      { label: "证券账户A", value: "acc-1" },
    ]);
    expect(selectedAccountId.value).toBe("acc-arch");
  });
});

describe("useRealizedPnl 标的筛选的选中回显接线（投影与钉住合并域断言归 useInstrumentOptions 单测）", () => {
  it("选中标的不在新候选时合并不丢（下拉不丢已选筛选）", async () => {
    vi.useFakeTimers();
    try {
      let calls = 0;
      wireInvokeSeam({
        overrides: {
          ...REFERENCE_OVERRIDES,
          list_instruments: () => {
            calls += 1;
            return calls === 1
              ? Promise.resolve({
                  items: [makeInstrument({ id: "inst-a", symbol: "AAA", name: "先选" })],
                  total: 1,
                })
              : Promise.resolve({
                  items: [makeInstrument({ id: "inst-b", symbol: "BBB", name: "后搜" })],
                  total: 1,
                });
          },
        },
      });
      const { searchInstruments, onSelectInstrument, pnlInstrumentOptions } = withSetup(() =>
        useRealizedPnl(),
      );
      searchInstruments("AAA");
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS);
      await flushPromises();
      onSelectInstrument("inst-a");
      await flushPromises();

      searchInstruments("BBB");
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS);
      await flushPromises();

      expect(pnlInstrumentOptions.value.map((o) => o.value)).toEqual(["inst-b", "inst-a"]);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe("useRealizedPnl 失败治愈（issue #325 Loadable 薄壳化）", () => {
  it("刷新失败不向调用方抛出：error 置位、loading 收尾、summary 保持原值不清空", async () => {
    const { summary, loading, error, refresh } = withSetup(() => useRealizedPnl());
    await refresh();
    expect(summary.value).toEqual(mockSummary);

    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...REFERENCE_OVERRIDES,
        realized_pnl_summary: () => Promise.reject(new Error("数据库文件已锁定")),
      },
    });
    await expect(refresh()).resolves.not.toThrow();
    expect(loading.value).toBe(false);
    expect(error.value).toBe("数据库文件已锁定");
    expect(summary.value).toEqual(mockSummary);
  });

  it("失败弹默认 toast（serde 对象错误归一取 message），成功不弹——error 状态与 toast 双通道共存", async () => {
    const sink = makeFakeSink();
    registerToastSink(sink);
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...REFERENCE_OVERRIDES,
        realized_pnl_summary: () => Promise.reject({ kind: "db", message: "盈亏汇总查询失败" }),
      },
    });
    const { error, refresh } = withSetup(() => useRealizedPnl());
    await flushPromises();
    expect(error.value).toBe("盈亏汇总查询失败");
    expect(sink.error).toHaveBeenCalledTimes(1);
    expect(sink.error).toHaveBeenCalledWith("盈亏汇总查询失败");

    wireInvokeSeam({ defaults: BASE_DEFAULTS, overrides: REFERENCE_OVERRIDES });
    await refresh();
    expect(sink.error).toHaveBeenCalledTimes(1);
  });

  it("失败后重试成功：error 清零、summary 重新填充（error 是唯一成败判据）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...REFERENCE_OVERRIDES,
        realized_pnl_summary: () => Promise.reject("首刷失败"),
      },
    });
    const { summary, error, refresh } = withSetup(() => useRealizedPnl());
    await refresh();
    expect(error.value).toBe("首刷失败");
    expect(summary.value).toBeNull();

    wireInvokeSeam({ defaults: BASE_DEFAULTS, overrides: REFERENCE_OVERRIDES });
    await refresh();
    expect(error.value).toBeNull();
    expect(summary.value).toEqual(mockSummary);
  });

  it("挂载首刷失败（onMounted 自动首刷）：不再产生未处理 rejection，进入 error 终态并弹 toast", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...REFERENCE_OVERRIDES,
        realized_pnl_summary: () => Promise.reject(new Error("首刷失败")),
      },
    });
    const sink = makeFakeSink();
    registerToastSink(sink);
    const wrapper = mount(Host);
    await flushPromises();
    expect(wrapper.vm.shell.error.value).toBe("首刷失败");
    expect(wrapper.vm.shell.summary.value).toBeNull();
    expect(sink.error).toHaveBeenCalledWith("首刷失败");
  });
});

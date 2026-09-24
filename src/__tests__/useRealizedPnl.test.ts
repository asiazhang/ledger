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

/** 默认 invoke 布线：已实现盈亏汇总 + 标的搜索契约快照（参考字典命令走接缝内建兜底） */
const BASE_DEFAULTS = {
  realized_pnl_summary: mockSummary,
  list_instruments: { items: [makeInstrument({ id: "inst-1" })], total: 1 },
};

/** 参考命令本场景需自定义值（overrides 优先于参考兜底）：账户选项断言消费「证券账户A」 */
const REFERENCE_OVERRIDES = { list_accounts: mockAccounts };

/** 宿主组件：模拟盈亏页在 setup 内使用 composable（onMounted 自动首刷时序留在薄壳内） */
const Host = defineComponent({
  setup() {
    return { shell: useRealizedPnl() };
  },
  template: "<div />",
});

beforeEach(async () => {
  wireInvokeSeam({ defaults: BASE_DEFAULTS, overrides: REFERENCE_OVERRIDES });
  // 每用例复位为 no-op，模拟「注册前」默认态，防模块级 sink 状态串扰
  resetToastSink();
  const store = useReferenceStore();
  await store.refresh();
});

describe("useRealizedPnl 已实现盈亏数据层", () => {
  it("加载已实现盈亏汇总（盈亏页两表的同一数据源）", async () => {
    const { summary, loading, refresh } = withSetup(() => useRealizedPnl());
    expect(summary.value).toBeNull(); // 未加载前空态
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
    // 同值重设不是实际变化：不触发归零（持仓翻页归零同规）
    session.setPnlYearPage(2);
    selectedAccountId.value = "acc-1";
    expect(session.pnlYearPage).toBe(2);
    // 标的筛选同规
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
            // 先发慢请求：手动放行，制造「后发已终态、先发迟到」的交错
            return new Promise<RealizedPnlSummary>((resolve) => {
              releaseFirst = resolve;
            });
          }
          return Promise.resolve(
            makePnlSummary({ total: [{ currency_code: "CNY", realized_pnl_cents: 777 }] }),
          );
        },
      },
    });
    const { summary, error, refresh } = withSetup(() => useRealizedPnl());
    const first = refresh();
    const second = refresh();
    await second;
    expect(summary.value!.total[0].realized_pnl_cents).toBe(777);

    // 迟到的先发结果：已被 Loadable 竞态裁决作废为空，不覆写终态、不置 error
    releaseFirst(mockSummary);
    await first;
    expect(summary.value!.total[0].realized_pnl_cents).toBe(777);
    expect(error.value).toBeNull();
  });

  it("账户/标的筛选生效：refresh 闭包自读当前筛选（0 元闭包，发起时点即最新值）", async () => {
    const { selectedAccountId, selectedInstrumentId, refresh } = withSetup(() => useRealizedPnl());
    await refresh();

    selectedAccountId.value = "acc-1";
    await refresh();
    // 调用序：[0] 挂载自动首刷（无筛选）、[1] 无筛选显式刷、[2] 账户筛选、[3] 双筛选
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
    // 挂载自动首刷 + onSelectInstrument 即时刷新
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

describe("useRealizedPnl 标的搜索候选投影（编排收口 useInstrumentSearch，机制断言在其模块单测）", () => {
  it("搜索结果投影为下拉选项：「代码 · 名称」label、id 为 value", async () => {
    vi.useFakeTimers();
    try {
      const { searchInstruments, pnlInstrumentOptions } = withSetup(() => useRealizedPnl());
      searchInstruments("浦发");
      await vi.advanceTimersByTimeAsync(SEARCH_DEBOUNCE_MS);
      await flushPromises();
      expect(pnlInstrumentOptions.value).toEqual([{ label: "600000 · 浦发银行", value: "inst-1" }]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("选中标的不在新候选时合并不丢（下拉不丢已选筛选）", async () => {
    vi.useFakeTimers();
    try {
      // 按次分支属动态行为：走 overrides 表（defaults 只收静态快照）
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

      // 新候选只有 inst-b，已选 inst-a 仍合并在选项尾部不丢
      expect(pnlInstrumentOptions.value).toEqual([
        { label: "BBB · 后搜", value: "inst-b" },
        { label: "AAA · 先选", value: "inst-a" },
      ]);
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
    // 挂载自动首刷已吃到拒绝布线并弹一次 toast，无需再显式首刷
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

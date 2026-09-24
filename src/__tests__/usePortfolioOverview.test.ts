import { describe, it, expect, beforeEach } from "vitest";
import { mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { mount, flushPromises } from "@vue/test-utils";
import { withSetup } from "@ledger/test-support/mount";
import { defineComponent } from "vue";
import { useReferenceStore } from "@/stores/reference";
import { registerToastSink } from "@ledger/loadable";
import {
  rowStatCardValues,
  usePortfolioOverview,
  type PortfolioRow,
} from "@/investment/usePortfolioOverview";
import {
  makeFakeSink,
  mockAccounts,
  mockHoldings,
  mockInstruments,
  resetToastSink,
} from "./factories";

/** 默认 invoke 布线：持仓 + 持仓标的字典契约快照（参考字典命令走接缝内建兜底） */
const BASE_DEFAULTS = {
  list_holdings: mockHoldings,
  list_instruments: { items: mockInstruments, total: mockInstruments.length },
  // 累计收益·折本位币单值（issue #1797）：后端三腿逐行折算聚合，前端透传
  cumulative_pnl_native_total: { total_cents: 48000, native_currency: "CNY" },
};

/** 参考命令本场景需自定义值（overrides 优先于参考兜底）：行装配断言消费账户名「证券账户A」 */
const REFERENCE_OVERRIDES = { list_accounts: mockAccounts };

/** 宿主组件：模拟盈亏页/首页在 setup 内使用 composable（onMounted 自动首刷时序留在薄壳内） */
const Host = defineComponent({
  setup() {
    return { shell: usePortfolioOverview() };
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

// ---------------------------------------------------------------------------
// rowStatCardValues：行集 → 市值/收益两卡单值装配（缺料三态分离的唯一表达式，
// issue #1797）。纯函数直测三态闭集；composable 集成口径见下一组。
// ---------------------------------------------------------------------------

/** 行夹具：只带两卡消费的两列（其余字段与装配无关） */
function row(partial: Partial<PortfolioRow> & { holdingId: string }): PortfolioRow {
  return {
    accountId: "acc-1",
    accountName: "证券账户A",
    instrumentId: `inst-${partial.holdingId}`,
    symbol: null,
    instrumentName: null,
    quantity: 100,
    costBasisCents: 100000,
    costCurrencyCode: "CNY",
    latestPriceCents: null,
    latestPriceCurrencyCode: null,
    latestNavDate: null,
    marketValueCents: null,
    unrealizedPnlCents: null,
    nativeMarketValueCents: null,
    nativeUnrealizedPnlCents: null,
    valueCurrencyCode: "CNY",
    priceChannel: "quote",
    ...partial,
  };
}

describe("rowStatCardValues 行集 → 单值装配（issue #1797）", () => {
  it("缺现价行（cents null）计入 missingPriceCount、不计入合计", () => {
    const { marketValue } = rowStatCardValues([
      row({ holdingId: "h1", marketValueCents: 150000, nativeMarketValueCents: 150000 }),
      row({ holdingId: "h2" }),
    ]);
    expect(marketValue.cents).toBe(150000);
    expect(marketValue.missingPriceCount).toBe(1);
    expect(marketValue.rateMissingCount).toBe(0);
    expect(marketValue.error).toBeNull();
  });

  it("有金额但缺折算汇率的行（nativeCents null）计入 rateMissingCount，不给半截数字", () => {
    const { unrealizedPnl } = rowStatCardValues([
      row({ holdingId: "h1", unrealizedPnlCents: 30000, nativeUnrealizedPnlCents: null }),
      row({ holdingId: "h2", unrealizedPnlCents: 10000, nativeUnrealizedPnlCents: 10000 }),
    ]);
    // 缺汇率行整卡警告：合计不为任何部分和
    expect(unrealizedPnl.cents).toBeNull();
    expect(unrealizedPnl.rateMissingCount).toBe(1);
    expect(unrealizedPnl.missingPriceCount).toBe(0);
  });

  it("同币（DefaultCurrency）行直接相加", () => {
    const { marketValue } = rowStatCardValues([
      row({ holdingId: "h1", marketValueCents: 150000, nativeMarketValueCents: 150000 }),
      row({ holdingId: "h2", marketValueCents: 2000000, nativeMarketValueCents: 1850000 }),
    ]);
    expect(marketValue.cents).toBe(2000000);
    expect(marketValue.missingPriceCount).toBe(0);
    expect(marketValue.rateMissingCount).toBe(0);
  });

  it("空行集：无可计入行（cents null、计数归零）", () => {
    const { marketValue, unrealizedPnl } = rowStatCardValues([]);
    expect(marketValue.cents).toBeNull();
    expect(unrealizedPnl.cents).toBeNull();
    expect(marketValue.missingPriceCount).toBe(0);
    expect(marketValue.rateMissingCount).toBe(0);
  });
});

describe("usePortfolioOverview 盈亏页持仓概览数据层（issue #110）", () => {
  it("加载持仓并与持仓标的字典/账户信息拼装成行（含折本位币两列透传）", async () => {
    const { rows, loading, refresh } = withSetup(() => usePortfolioOverview());
    await refresh();
    expect(loading.value).toBe(false);
    expect(rows.value.length).toBe(2);
    const row1 = rows.value[0]!;
    expect(row1.symbol).toBe("600000");
    expect(row1.instrumentName).toBe("浦发银行");
    expect(row1.accountName).toBe("证券账户A");
    expect(row1.quantity).toBe(100);
    expect(row1.costBasisCents).toBe(120000);
    expect(row1.latestPriceCents).toBe(150000); // 现价为万分之一元刻度（ADR-0038）
    expect(row1.marketValueCents).toBe(150000);
    expect(row1.unrealizedPnlCents).toBe(30000);
    // 折本位币两列随行透传（后端逐行当期汇率软折算，issue #1797），合计卡消费
    expect(row1.nativeMarketValueCents).toBe(150000);
    expect(row1.nativeUnrealizedPnlCents).toBe(30000);
    // 市值/未实现盈亏折算币种 = 账户币
    expect(row1.valueCurrencyCode).toBe("CNY");
  });

  it("查询持仓标的字典时携带 only_invested=true（与增量同步同口径）", async () => {
    const { refresh } = withSetup(() => usePortfolioOverview());
    await refresh();
    const call = mockInvoke.mock.calls.find(([cmd]) => cmd === "list_instruments");
    expect(call![1]).toMatchObject({ filter: { only_invested: true } });
  });

  it("净值日期透传到行（基金现价对应哪天的净值，#303）", async () => {
    const { rows, refresh } = withSetup(() => usePortfolioOverview());
    await refresh();
    // 默认夹具为股票行：latest_nav_date 为 null
    expect(rows.value[0]!.latestNavDate).toBeNull();
  });

  it("合计单值（issue #1797）：缺现价行未计入并计数，折本位币列求和", async () => {
    const { statCards, refresh } = withSetup(() => usePortfolioOverview());
    await refresh();
    // h-2 无行情 NULL 不计入且计数；只有 h-1 计入（150000 / 30000）
    expect(statCards.value.marketValue).toEqual({
      cents: 150000,
      missingPriceCount: 1,
      rateMissingCount: 0,
      error: null,
    });
    expect(statCards.value.unrealizedPnl).toEqual({
      cents: 30000,
      missingPriceCount: 1,
      rateMissingCount: 0,
      error: null,
    });
  });

  it("累计收益透传后端折本位币单值（issue #1797）：不从持仓行派生，nativeCurrency 随命令", async () => {
    const { cumulativePnl, nativeCurrency, refresh } = withSetup(() => usePortfolioOverview());
    await refresh();
    expect(cumulativePnl.value).toEqual({
      cents: 48000,
      missingPriceCount: 0,
      rateMissingCount: 0,
      error: null,
    });
    expect(nativeCurrency.value).toBe("CNY");
    // 单值只可能来自后端命令（三腿折算聚合在前端不可复算）
    expect(mockInvoke.mock.calls.some(([c]) => c === "cumulative_pnl_native_total")).toBe(true);
  });

  it("无持仓时行为明确：rows 为空、合计无计入行（cents null），不报错", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...REFERENCE_OVERRIDES,
        list_holdings: [],
        list_instruments: { items: [], total: 0 },
      },
    });
    const { rows, loading, statCards, refresh } = withSetup(() => usePortfolioOverview());
    await refresh();
    expect(rows.value).toEqual([]);
    expect(statCards.value.marketValue.cents).toBeNull();
    expect(statCards.value.unrealizedPnl.cents).toBeNull();
    expect(statCards.value.marketValue.missingPriceCount).toBe(0);
    expect(loading.value).toBe(false);
  });

  it("两路读独立（issue #1797）：累计收益命令失败只降级该卡（警告态），不拖累持仓行装配", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...REFERENCE_OVERRIDES,
        cumulative_pnl_native_total: () => Promise.reject(new Error("缺少 USD→CNY 汇率，无法折算")),
      },
    });
    const { rows, cumulativePnl, refresh } = withSetup(() => usePortfolioOverview());
    await refresh();
    // 持仓行不受拖累（表格的账户币展示不依赖折算）
    expect(rows.value.length).toBe(2);
    // 累计收益卡进入警告态：报错文案置位、不给半截数字
    expect(cumulativePnl.value.error).toBe("缺少 USD→CNY 汇率，无法折算");
    expect(cumulativePnl.value.cents).toBeNull();

    // 重试成功即恢复（重试 = 重发同一条读命令）
    wireInvokeSeam({ defaults: BASE_DEFAULTS, overrides: REFERENCE_OVERRIDES });
    await refresh();
    expect(cumulativePnl.value.error).toBeNull();
    expect(cumulativePnl.value.cents).toBe(48000);
  });
});

describe("usePortfolioOverview 失败治愈（issue #324 Loadable 薄壳化）", () => {
  it("刷新失败不向调用方抛出：error 置位、loading 收尾、rows 保持原值不清空", async () => {
    const { rows, loading, error, refresh } = withSetup(() => usePortfolioOverview());
    await refresh();
    expect(rows.value.length).toBe(2);

    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...REFERENCE_OVERRIDES,
        list_holdings: () => Promise.reject(new Error("数据库文件已锁定")),
      },
    });
    await expect(refresh()).resolves.not.toThrow();
    expect(loading.value).toBe(false);
    expect(error.value).toBe("数据库文件已锁定");
    expect(rows.value.length).toBe(2);
  });

  it("失败弹默认 toast（serde 对象错误归一取 message），成功不弹——error 状态与 toast 双通道共存", async () => {
    const sink = makeFakeSink();
    registerToastSink(sink);
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...REFERENCE_OVERRIDES,
        list_holdings: () => Promise.reject({ kind: "db", message: "持仓查询失败" }),
      },
    });
    // 挂载自动首刷已吃到拒绝布线并弹一次 toast，无需再显式首刷
    const { error, refresh } = withSetup(() => usePortfolioOverview());
    await flushPromises();
    expect(error.value).toBe("持仓查询失败");
    expect(sink.error).toHaveBeenCalledTimes(1);
    expect(sink.error).toHaveBeenCalledWith("持仓查询失败");

    wireInvokeSeam({ defaults: BASE_DEFAULTS, overrides: REFERENCE_OVERRIDES });
    await refresh();
    expect(sink.error).toHaveBeenCalledTimes(1);
  });

  it("失败后重试成功：error 清零、rows 重新装配（error 是唯一成败判据）", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: { ...REFERENCE_OVERRIDES, list_holdings: () => Promise.reject("首刷失败") },
    });
    const { rows, error, refresh } = withSetup(() => usePortfolioOverview());
    await refresh();
    expect(error.value).toBe("首刷失败");
    expect(rows.value).toEqual([]);

    wireInvokeSeam({ defaults: BASE_DEFAULTS, overrides: REFERENCE_OVERRIDES });
    await refresh();
    expect(error.value).toBeNull();
    expect(rows.value.length).toBe(2);
  });

  it("挂载首刷失败（onMounted 自动首刷）：不再产生未处理 rejection，进入 error 终态并弹 toast", async () => {
    wireInvokeSeam({
      defaults: BASE_DEFAULTS,
      overrides: {
        ...REFERENCE_OVERRIDES,
        list_holdings: () => Promise.reject(new Error("首刷失败")),
      },
    });
    const sink = makeFakeSink();
    registerToastSink(sink);
    const wrapper = mount(Host);
    await flushPromises();
    expect(wrapper.vm.shell.error.value).toBe("首刷失败");
    expect(wrapper.vm.shell.rows.value).toEqual([]);
    expect(sink.error).toHaveBeenCalledWith("首刷失败");
  });
});

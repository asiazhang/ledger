import { describe, it, expect, vi, afterEach } from "vitest";
import { setActivePinia, createPinia } from "pinia";
import { SEARCH_DEBOUNCE_MS } from "@/composables/search-debounce";
import {
  LEDGER_TAB_PAGE_SIZE_DEFAULT,
  TREND_MODE_DEFAULT,
  TREND_PRESET_DEFAULT,
  useInvestmentsSessionStore,
} from "@/investment/investments-session";
import { makeInstrument } from "./factories";

afterEach(() => {
  vi.useRealTimers();
});

describe("useInvestmentsSessionStore（issue #1192 投资页会话状态）", () => {
  it("冷启动默认：默认页签「概览」、无持仓筛选/排序、第 1 页、组合走势默认区间", () => {
    const store = useInvestmentsSessionStore();
    expect(store.activeTab).toBe("overview");
    expect(store.holdingsSearchInput).toBe("");
    expect(store.holdingsSearch).toBe("");
    expect(store.holdingsAccountId).toBeNull();
    expect(store.holdingsSorter).toBeNull();
    expect(store.holdingsPage).toBe(1);
    expect(store.trendMode).toBe(TREND_MODE_DEFAULT);
    expect(store.trendPreset).toBe(TREND_PRESET_DEFAULT);
    expect(store.trendInstrumentId).toBeNull();
    expect(store.trendInstrument).toBeNull();
  });

  it("会话内保留、冷启动回默认（新 pinia 回默认，同 pinia 保留）", () => {
    const store = useInvestmentsSessionStore();
    store.setActiveTab("holdings");
    store.setAccount("acc-1");
    store.setSorter({ columnKey: "market_value", order: "descend" });
    store.setPage(3);
    expect(useInvestmentsSessionStore().activeTab).toBe("holdings");
    expect(useInvestmentsSessionStore().holdingsAccountId).toBe("acc-1");
    expect(useInvestmentsSessionStore().holdingsSorter).toEqual({
      columnKey: "market_value",
      order: "descend",
    });
    expect(useInvestmentsSessionStore().holdingsPage).toBe(3);

    // 新 pinia = 冷启动：全部回默认
    setActivePinia(createPinia());
    const cold = useInvestmentsSessionStore();
    expect(cold.activeTab).toBe("overview");
    expect(cold.holdingsAccountId).toBeNull();
    expect(cold.holdingsSorter).toBeNull();
    expect(cold.holdingsPage).toBe(1);
  });

  it("搜索防抖：输入回显即时、应用值 300ms 后生效；翻页归零随应用时点", () => {
    vi.useFakeTimers();
    const store = useInvestmentsSessionStore();
    store.setPage(2);
    store.setSearch("600");
    expect(store.holdingsSearchInput).toBe("600");
    // 防抖窗口内应用值未变、页码不归零
    expect(store.holdingsSearch).toBe("");
    expect(store.holdingsPage).toBe(2);
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS);
    expect(store.holdingsSearch).toBe("600");
    expect(store.holdingsPage).toBe(1);
  });

  it("排序同值重设不归零；实际变化（含第三态清除）归零第一页", () => {
    const store = useInvestmentsSessionStore();
    store.setSorter({ columnKey: "unrealized_pnl", order: "ascend" });
    store.setPage(2);
    store.setSorter({ columnKey: "unrealized_pnl", order: "ascend" });
    expect(store.holdingsPage).toBe(2);
    // order=false（naive-ui 第三态）清除排序：属实际变化
    store.setSorter({ columnKey: "unrealized_pnl", order: false });
    expect(store.holdingsSorter).toBeNull();
    expect(store.holdingsPage).toBe(1);
  });

  it("走势入口：带入标的即选中并切单标的模式；投影可读、面板下拉切换同理", () => {
    const store = useInvestmentsSessionStore();
    const inst = makeInstrument({ id: "inst-1", symbol: "600000", name: "浦发银行" });
    store.showTrendInstrument(inst);
    expect(store.trendInstrumentId).toBe("inst-1");
    expect(store.trendMode).toBe("instrument");
    expect(store.trendInstrument?.symbol).toBe("600000");
  });

  it("selectTrendInstrument(null) 清除选中并回组合模式（面板清除/切换出口）", () => {
    const store = useInvestmentsSessionStore();
    store.showTrendInstrument(makeInstrument({ id: "inst-1" }));
    store.selectTrendInstrument(null);
    expect(store.trendInstrumentId).toBeNull();
    expect(store.trendMode).toBe(TREND_MODE_DEFAULT);
    expect(store.trendInstrument).toBeNull();
  });
});

describe("resetToDefault（issue #1192 ESC 复位出口）", () => {
  it("偏离的页签/筛选/排序/页码/走势/明细筛选全部回默认（清除保留态本身）", () => {
    const store = useInvestmentsSessionStore();
    store.setActiveTab("trend");
    store.setSearch("600");
    store.setAccount("acc-1");
    store.setSorter({ columnKey: "market_value", order: "descend" });
    store.setPage(3);
    store.setPnlYearPage(3);
    store.setPnlAccountPage(2);
    store.showTrendInstrument(makeInstrument({ id: "inst-1" }));
    store.setTrendPreset("1m");
    store.setDetailKinds(["buy", "sell"]);
    store.setDetailPage(2);
    store.setDetailPageSize(50);
    store.setDetailAccount("acc-2");
    store.setDetailInstrument("inst-1");
    store.setDetailDateRange(["2026-01-01", "2026-03-31"]);
    store.resetToDefault();
    expect(store.activeTab).toBe("overview");
    expect(store.holdingsSearchInput).toBe("");
    expect(store.holdingsSearch).toBe("");
    expect(store.holdingsAccountId).toBeNull();
    expect(store.holdingsSorter).toBeNull();
    expect(store.holdingsPage).toBe(1);
    expect(store.pnlYearPage).toBe(1);
    expect(store.pnlAccountPage).toBe(1);
    expect(store.trendMode).toBe(TREND_MODE_DEFAULT);
    expect(store.trendPreset).toBe(TREND_PRESET_DEFAULT);
    expect(store.trendInstrumentId).toBeNull();
    expect(store.trendInstrument).toBeNull();
    expect(store.detailKinds).toBeNull();
    expect(store.detailPage).toBe(1);
    expect(store.detailPageSize).toBe(LEDGER_TAB_PAGE_SIZE_DEFAULT);
    expect(store.detailAccountId).toBeNull();
    expect(store.detailInstrumentId).toBeNull();
    expect(store.detailDateFrom).toBeNull();
    expect(store.detailDateTo).toBeNull();
  });

  it("复位撤销在途搜索防抖：复位后旧输入不落地（不留迟到写入）", () => {
    vi.useFakeTimers();
    const store = useInvestmentsSessionStore();
    store.setSearch("600");
    store.resetToDefault();
    // 防抖窗口推进后应用值仍为空：复位已撤销在途定时器
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS * 2);
    expect(store.holdingsSearch).toBe("");
    expect(store.holdingsSearchInput).toBe("");
  });

  it("复位语义优先：已应用搜索时 ESC 复位后回显与应用值同为默认空、定时器已撤销", () => {
    vi.useFakeTimers();
    const store = useInvestmentsSessionStore();
    // 先应用一个搜索（回显 = 应用值 = '600'）
    store.setSearch("600");
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS);
    expect(store.holdingsSearch).toBe("600");
    expect(store.holdingsSearchInput).toBe("600");
    store.resetToDefault();
    // 复位即回默认：回显不得残留旧应用值
    expect(store.holdingsSearch).toBe("");
    expect(store.holdingsSearchInput).toBe("");
    // 复位后再输入一个值但未到防抖窗口，再复位：旧输入不落地
    store.setSearch("000001");
    store.resetToDefault();
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS * 2);
    expect(store.holdingsSearch).toBe("");
    expect(store.holdingsSearchInput).toBe("");
  });

  it("cancelPendingSearch 撤销在途防抖：应用值不动、回显回到应用值（离开视图语义）", () => {
    vi.useFakeTimers();
    const store = useInvestmentsSessionStore();
    store.setSearch("600");
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS);
    expect(store.holdingsSearch).toBe("600");
    // 已应用后再输入但未到防抖窗口：离开视图
    store.setSearch("600000");
    expect(store.holdingsSearchInput).toBe("600000");
    store.cancelPendingSearch();
    vi.advanceTimersByTime(SEARCH_DEBOUNCE_MS * 2);
    // 未应用的新输入被撤销，应用值与回显一致地停在最后应用值
    expect(store.holdingsSearch).toBe("600");
    expect(store.holdingsSearchInput).toBe("600");
  });

  it("默认态复位幂等：全默认时复位无副作用", () => {
    const store = useInvestmentsSessionStore();
    store.resetToDefault();
    expect(store.activeTab).toBe("overview");
    expect(store.holdingsPage).toBe(1);
    expect(store.trendMode).toBe(TREND_MODE_DEFAULT);
    expect(store.trendInstrument).toBeNull();
  });
});

describe("store 写路径唯一（issue #1192 Standards 轴 finding）", () => {
  it("全部状态变化都经意图入口（只读投影 + 入口动作是唯一写路）", () => {
    const store = useInvestmentsSessionStore();
    expect(store.activeTab).toBe("overview");
    store.setActiveTab("holdings");
    expect(store.activeTab).toBe("holdings");
    expect(store.trendPreset).toBe(TREND_PRESET_DEFAULT);
    store.setTrendPreset("1m");
    expect(store.trendPreset).toBe("1m");
  });

  it('单标的模式守卫：无选中标的时 setTrendMode("instrument") 无操作', () => {
    const store = useInvestmentsSessionStore();
    store.setTrendMode("instrument");
    expect(store.trendMode).toBe(TREND_MODE_DEFAULT);
    store.showTrendInstrument(makeInstrument({ id: "inst-1" }));
    store.setTrendMode("portfolio");
    store.setTrendMode("instrument");
    expect(store.trendMode).toBe("instrument");
  });

  it("selectTrendInstrument 只接受已声明的标的 id（未知 id 无操作）", () => {
    const store = useInvestmentsSessionStore();
    store.selectTrendInstrument("inst-unknown");
    expect(store.trendInstrumentId).toBeNull();
    store.showTrendInstrument(makeInstrument({ id: "inst-1" }));
    store.selectTrendInstrument("inst-1");
    expect(store.trendInstrumentId).toBe("inst-1");
  });
});

/**
 * 明细页签筛选与分页（ADR-0135 决策 3 / issue #1779）：类型多选筛选与页码/页大小
 * 入投资页会话 store（会话内保留、冷启动回默认，ADR-0094 默认粒度）。语义与主列表
 * 类型维度同构：空集合 ≡ 不过滤 ≡ 默认态；维度实际变化翻页归零；页大小切换翻回第 1 页。
 */
describe("明细页签筛选与分页（ADR-0135 / issue #1779）", () => {
  it("冷启动默认：无类型筛选、第 1 页、默认页大小", () => {
    const store = useInvestmentsSessionStore();
    expect(store.detailKinds).toBeNull();
    expect(store.detailPage).toBe(1);
    expect(store.detailPageSize).toBe(LEDGER_TAB_PAGE_SIZE_DEFAULT);
  });

  it("类型筛选写入：空集合归一为 null（空集合 ≡ 不过滤 ≡ 默认态）", () => {
    const store = useInvestmentsSessionStore();
    store.setDetailKinds(["buy", "dividend"]);
    expect(store.detailKinds).toEqual(["buy", "dividend"]);
    store.setDetailKinds([]);
    expect(store.detailKinds).toBeNull();
    store.setDetailKinds(null);
    expect(store.detailKinds).toBeNull();
  });

  it("类型筛选实际变化翻页归零；同值重设不归零（顺序无关）", () => {
    const store = useInvestmentsSessionStore();
    store.setDetailKinds(["buy"]);
    store.setDetailPage(3);
    store.setDetailKinds(["sell"]);
    expect(store.detailPage).toBe(1);
    // 集合实际变化（扩大）→ 翻页归零
    store.setDetailKinds(["buy", "sell"]);
    expect(store.detailPage).toBe(1);
    store.setDetailPage(2);
    // 同一集合不同顺序：同值重设，不翻页归零
    store.setDetailKinds(["sell", "buy"]);
    // 同值守卫不动作：保留态保持首写顺序，也不触发翻页归零
    expect(store.detailKinds).toEqual(["buy", "sell"]);
    expect(store.detailPage).toBe(2);
  });

  it("页大小切换翻回第 1 页（主列表同构）", () => {
    const store = useInvestmentsSessionStore();
    store.setDetailPage(3);
    store.setDetailPageSize(50);
    expect(store.detailPageSize).toBe(50);
    expect(store.detailPage).toBe(1);
  });
});

/**
 * 明细页签筛选全维（issue #1780 / ADR-0135 决策 3）：账户（涉及账户语义——账户端 ∪
 * 出资端）、标的（convert 两腿任一命中即算）与日期（双端有界，picker 成对写入/清除）
 * 与类型同住会话 store：写入入口、同值幂等、任一维度实际变化翻页归零、会话内保留、
 * 冷启动回默认、ESC 复位清零。
 */
describe("明细页签筛选全维（issue #1780：账户/标的/日期）", () => {
  it("冷启动默认：三维均为 null（无筛选）", () => {
    const store = useInvestmentsSessionStore();
    expect(store.detailAccountId).toBeNull();
    expect(store.detailInstrumentId).toBeNull();
    expect(store.detailDateFrom).toBeNull();
    expect(store.detailDateTo).toBeNull();
  });

  it("三维写入：会话内保留（同 pinia 跨 store 读取可见）", () => {
    const store = useInvestmentsSessionStore();
    store.setDetailAccount("acc-2");
    store.setDetailInstrument("inst-1");
    store.setDetailDateRange(["2026-01-01", "2026-03-31"]);
    const again = useInvestmentsSessionStore();
    expect(again.detailAccountId).toBe("acc-2");
    expect(again.detailInstrumentId).toBe("inst-1");
    expect(again.detailDateFrom).toBe("2026-01-01");
    expect(again.detailDateTo).toBe("2026-03-31");
    // 新 pinia = 冷启动：回默认
    setActivePinia(createPinia());
    const cold = useInvestmentsSessionStore();
    expect(cold.detailAccountId).toBeNull();
    expect(cold.detailInstrumentId).toBeNull();
    expect(cold.detailDateFrom).toBeNull();
    expect(cold.detailDateTo).toBeNull();
  });

  it("同值重设幂等不动作；空集合/空区间归一默认", () => {
    const store = useInvestmentsSessionStore();
    store.setDetailAccount("acc-2");
    store.setDetailAccount("acc-2");
    expect(store.detailAccountId).toBe("acc-2");
    store.setDetailInstrument("inst-1");
    store.setDetailInstrument("inst-1");
    expect(store.detailInstrumentId).toBe("inst-1");
    store.setDetailDateRange(["2026-01-01", "2026-03-31"]);
    // 同区间重写不动作（picker 受控回显重放同值）
    store.setDetailDateRange(["2026-01-01", "2026-03-31"]);
    expect(store.detailDateFrom).toBe("2026-01-01");
    expect(store.detailDateTo).toBe("2026-03-31");
    // 清除：null 回默认
    store.setDetailDateRange(null);
    expect(store.detailDateFrom).toBeNull();
    expect(store.detailDateTo).toBeNull();
  });

  it("任一维度实际变化翻页归零；同值重设不归零", () => {
    const store = useInvestmentsSessionStore();
    store.setDetailAccount("acc-2");
    store.setDetailPage(3);
    store.setDetailInstrument("inst-1");
    expect(store.detailPage).toBe(1);
    store.setDetailPage(2);
    store.setDetailDateRange(["2026-01-01", "2026-03-31"]);
    expect(store.detailPage).toBe(1);
    store.setDetailPage(2);
    // 同值重设（账户/标的/区间）不归零
    store.setDetailAccount("acc-2");
    store.setDetailInstrument("inst-1");
    store.setDetailDateRange(["2026-01-01", "2026-03-31"]);
    expect(store.detailPage).toBe(2);
    // 清除账户也是实际变化 → 归零
    store.setDetailAccount(null);
    expect(store.detailPage).toBe(1);
  });
});

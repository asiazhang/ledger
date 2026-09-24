import { describe, it, expect, vi, beforeEach } from "vitest";
import { lastInvokeArgs, mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { NDataTable, NSelect } from "naive-ui";
import { createPinia, setActivePinia } from "pinia";
import InvestmentsView from "@/views/InvestmentsView.vue";
import InvestmentLedgerTab from "@/investment/InvestmentLedgerTab.vue";
import { useInvestmentsSessionStore } from "@/investment/investments-session";
import { formatAmount, formatPrice, formatQuantity } from "@ledger/money";
import { clickTab } from "@ledger/test-support/dom";
import { componentVm } from "@ledger/test-support/component-vm";
import { mountWithDialog } from "@ledger/test-support/mount";
import { setFakeMedia } from "@ledger/test-support/media-mock";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import { makeInvestmentLedgerRow, makeInvestmentOverview, makeMwrSummary } from "../factories";
import type { InvestmentTransactionRow } from "@ledger/types";

// 走势图用共享桩组件替代（同 InvestmentsView.test.ts 基座）
vi.mock("vue-chartjs", async () => {
  const { LineChartStub } = await import("../line-chart-stub");
  return { Line: LineChartStub };
});

// 价格失效信号订阅 mock（同 InvestmentsView.test.ts 基座）
vi.mock("@/investment/usePricesChanged", async () => {
  const { capturePricesChangedHandler } = await import("../prices-changed-mock");
  return {
    usePricesChanged: (cb: () => void) => capturePricesChangedHandler(cb),
  };
});

// focus 参数读取自路由 query（同 InvestmentsView.test.ts 的可控 mockRoute 先例）
const pushMock = vi.fn();
const mockRoute = { query: {} as Record<string, string> };
vi.mock("vue-router", () => ({
  useRoute: () => mockRoute,
  useRouter: () => ({ push: pushMock }),
}));

/** 视图挂载走共享基座（NDialogProvider 包裹；useMessage 由 naive-ui 桩规范兜底）。 */
const mountView = () => mountWithDialog(InvestmentsView);

// —— 明细行夹具：五种投资 kind 各一行（date 倒序排布，与后端排序同构）——

/** 买入：10 @ 10 元 + 手续费 1 元，经出资账户「银行」直扣（ADR-0096）。 */
const buyRow = makeInvestmentLedgerRow({
  id: "t-buy",
  kind: "buy",
  date: "2026-03-05",
  amount_cents: 10000,
  funding_account_id: "acc-2",
  trade: { quantity: 10, price_cents: 100000, fee_cents: 100 },
});

/** 卖出：4 @ 12 元 + 手续费 0.5 元。 */
const sellRow = makeInvestmentLedgerRow({
  id: "t-sell",
  kind: "sell",
  date: "2026-03-04",
  amount_cents: 4800,
  trade: { quantity: 4, price_cents: 120000, fee_cents: 50 },
});

/** 转换 A → B：转出 2 份确认 200 元、转入 1 份确认 190 元，行金额锚点为结转成本。 */
const convertRow = makeInvestmentLedgerRow({
  id: "t-convert",
  kind: "convert",
  date: "2026-03-03",
  symbol: "AAPL",
  instrument_name: "苹果",
  amount_cents: 21000,
  convert: {
    quantity: 2,
    to_instrument_id: "inst-msft",
    to_symbol: "MSFT",
    to_quantity: 1,
    out_amount_cents: 20000,
    in_amount_cents: 19000,
  },
});

/** 份额调整：折算 +2 份（带符号增量 Δ，无现金腿）。 */
const splitRow = makeInvestmentLedgerRow({
  id: "t-split",
  kind: "split",
  date: "2026-03-02",
  symbol: "000001",
  instrument_name: "平安银行",
  amount_cents: 0,
  split: { delta_quantity: 2 },
});

/** 缩股：−3 份（负向增量 Δ）。 */
const shrinkRow = makeInvestmentLedgerRow({
  id: "t-shrink",
  kind: "split",
  date: "2026-03-01",
  symbol: "000001",
  instrument_name: "平安银行",
  amount_cents: 0,
  split: { delta_quantity: -3 },
});

/** 现金分红：300 元到账「银行」（账户端即到账账户），无数量/单价/手续费。 */
const dividendRow = makeInvestmentLedgerRow({
  id: "t-dividend",
  kind: "dividend",
  date: "2026-02-28",
  amount_cents: 30000,
  account_id: "acc-2",
  symbol: "AAPL",
  instrument_name: "苹果",
});

/** 25 行买入库：分页用例的行集（第 1 页 20 行 + 第 2 页 5 行）。 */
const manyBuyRows: InvestmentTransactionRow[] = Array.from({ length: 25 }, (_, i) =>
  makeInvestmentLedgerRow({
    id: `t-page-${String(i + 1).padStart(2, "0")}`,
    kind: "buy",
    date: `2026-02-${String(i + 1).padStart(2, "0")}`,
    trade: { quantity: i + 1, price_cents: 100000, fee_cents: 0 },
  }),
);

/** 可变明细库（行为编排型 overrides 读取最新值）。 */
let ledgerDb: InvestmentTransactionRow[] = [];

/** 与后端 ledger_tab 口径一致：kind 子集（维度内取或）+ offset 分页，total 恒返回。 */
function listInvestmentTransactions(args?: { filter?: Record<string, unknown> }) {
  const filter = args?.filter ?? {};
  const scoped = ledgerDb.filter(
    (r) => !Array.isArray(filter.kinds) || filter.kinds.includes(r.kind),
  );
  const pageSize = (filter.page_size as number) ?? scoped.length;
  const page = (filter.page as number) ?? 1;
  const start = (page - 1) * pageSize;
  return Promise.resolve({
    items: scoped.slice(start, start + pageSize),
    total: scoped.length,
  });
}

/** 投资域命令契约快照（与 InvestmentsView.test.ts 同款；明细行集见 ledgerDb）。 */
const LEDGER_DEFAULTS = {
  investment_overview: makeInvestmentOverview(),
  list_holdings: [],
  instrument_price_staleness: { stale_count: 0, threshold_days: 3 },
  cumulative_pnl_summary: [],
  portfolio_value_trend: { currency_code: "CNY", points: [] },
  instrument_price_trend: { instrument_id: "inst-1", points: [] },
  realized_pnl_summary: {
    total_realized_pnl_cents: 0,
    by_year: [],
    by_account: [],
    by_instrument: [],
    details: [],
  },
  money_weighted_return_summary: makeMwrSummary({ by_instrument: [], by_account: [], total: [] }),
};

beforeEach(async () => {
  mockRoute.query = {};
  ledgerDb = [buyRow, sellRow, convertRow, splitRow, dividendRow];
  await wireInvokeSeam({
    defaults: LEDGER_DEFAULTS,
    overrides: {
      // 账户字典：券商户（投资账户）+ 银行（出资/到账账户），名单测试可读
      list_accounts: () => [
        {
          id: "acc-1",
          name: "券商户",
          type: "investment",
          currency_code: "CNY",
          initial_balance_cents: 0,
          created_at: "2026-01-01T00:00:00Z",
          updated_at: "2026-01-01T00:00:00Z",
          version: 1,
          device_id: "test",
          is_deleted: false,
          is_hidden: false,
        },
        {
          id: "acc-2",
          name: "银行",
          type: "bank",
          currency_code: "CNY",
          initial_balance_cents: 0,
          created_at: "2026-01-01T00:00:00Z",
          updated_at: "2026-01-01T00:00:00Z",
          version: 1,
          device_id: "test",
          is_deleted: false,
          is_hidden: false,
        },
      ],
      list_investment_transactions: listInvestmentTransactions,
    },
    refreshReferenceStores: true,
  }).ready;
});

// —— 页签内查找助手（视图装配多表格，先定位明细页签组件再向其子树查找）——

function ledgerPane(wrapper: VueWrapper) {
  const pane = wrapper.findComponent(InvestmentLedgerTab);
  expect(pane.exists(), "明细页签组件应已挂载（先打开明细页签）").toBe(true);
  return pane;
}

function ledgerTable(wrapper: VueWrapper) {
  return ledgerPane(wrapper).findComponent(NDataTable);
}

/** 服务端分页装配缝（受控 pagination 对象的出口，TransactionsView 同款窄化）。 */
function tablePagination(wrapper: VueWrapper): {
  onChange: (page: number) => void;
  onUpdatePageSize: (pageSize: number) => void;
} {
  const p = ledgerTable(wrapper).props("pagination") as {
    onChange?: (page: number) => void;
    onUpdatePageSize?: (pageSize: number) => void;
  };
  return {
    onChange: (page: number) => p.onChange?.(page),
    onUpdatePageSize: (pageSize: number) => p.onUpdatePageSize?.(pageSize),
  };
}

function colCells(wrapper: VueWrapper, key: string) {
  return ledgerPane(wrapper)
    .findAll(`td[data-col-key="${key}"]`)
    .map((c) => c.text());
}

async function openLedgerTab(wrapper: VueWrapper) {
  await clickTab(wrapper, "明细");
  await flushPromises();
}

function ledgerCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === "list_investment_transactions");
}

function lastLedgerFilter(): Record<string, unknown> {
  return lastInvokeArgs("list_investment_transactions").filter as Record<string, unknown>;
}

/** 类型多选（AppSelect 装配缝直发 update:value，与用户下拉操作同一路径）。 */
async function selectKinds(wrapper: VueWrapper, values: string[] | null) {
  const select = ledgerPane(wrapper).findComponent(AppSelect);
  componentVm(select).$emit("update:value", values);
  await flushPromises();
}

/** 打开页签后类型筛选值投影（受控 :value 经 attrs 透传到 NSelect，读其声明 prop）。 */
function selectedKinds(wrapper: VueWrapper): string[] {
  const inner = ledgerPane(wrapper).findComponent(NSelect);
  return (inner.props("value") as string[] | null) ?? [];
}

/**
 * 明细页签基座——kind 分形态渲染（ADR-0135 决策 3 / issue #1779）：
 * 买卖行标的/数量/单价/手续费/出资账户、convert「A → B」双腿、split 带符号 Δ、
 * dividend 现金腿与到账账户；金额列走统一展示格式化（formatAmount/formatPrice）。
 * 断言对准用户可观察结果（单元格文本），删除任一形态接线即红。
 */
describe("明细页签 kind 分形态渲染（issue #1779）", () => {
  it("打开明细页签：五种 kind 按 date 倒序渲染，类型列呈现 kind 文案", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    expect(colCells(wrapper, "kind")).toEqual(["买入", "卖出", "转换", "份额调整", "分红"]);
    expect(colCells(wrapper, "date")).toEqual([
      "2026-03-05",
      "2026-03-04",
      "2026-03-03",
      "2026-03-02",
      "2026-02-28",
    ]);
  });

  it("买卖行：标的/数量/单价/手续费/金额，buy 行带出资账户、sell 行出资账户为「-」", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    // 买入（第 1 行）：标的 = 代码 + 名称，出资账户直扣户上屏
    expect(colCells(wrapper, "instrument")[0]).toBe("600000 浦发银行");
    expect(colCells(wrapper, "quantity")[0]).toBe(formatQuantity(10));
    expect(colCells(wrapper, "price")[0]).toBe(formatPrice(100000));
    expect(colCells(wrapper, "fee")[0]).toBe(formatAmount(100));
    expect(colCells(wrapper, "amount")[0]).toBe(formatAmount(10000));
    expect(colCells(wrapper, "funding_account")[0]).toBe("银行");
    // 卖出（第 2 行）
    expect(colCells(wrapper, "quantity")[1]).toBe(formatQuantity(4));
    expect(colCells(wrapper, "price")[1]).toBe(formatPrice(120000));
    expect(colCells(wrapper, "fee")[1]).toBe(formatAmount(50));
    expect(colCells(wrapper, "amount")[1]).toBe(formatAmount(4800));
    expect(colCells(wrapper, "funding_account")[1]).toBe("-");
  });

  it("转换行：「A → B」双腿标的、转出 → 转入份额双腿，金额读转出金额，无单价/手续费", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    // 转换行（第 3 行）
    expect(colCells(wrapper, "instrument")[2]).toBe("AAPL → MSFT");
    expect(colCells(wrapper, "quantity")[2]).toBe(`${formatQuantity(2)} → ${formatQuantity(1)}`);
    // 展示金额读转换载荷转出金额（与主列表同口径），行金额锚点（结转成本）不上屏
    expect(colCells(wrapper, "amount")[2]).toBe(formatAmount(20000));
    expect(colCells(wrapper, "price")[2]).toBe("-");
    expect(colCells(wrapper, "fee")[2]).toBe("-");
  });

  it("份额调整行：带符号份额增量 Δ（折算 +、缩股 −），无现金腿金额为「-」", async () => {
    ledgerDb = [splitRow, shrinkRow];
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    expect(colCells(wrapper, "quantity")).toEqual([`+${formatQuantity(2)}`, formatQuantity(-3)]);
    expect(colCells(wrapper, "amount")).toEqual(["-", "-"]);
    expect(colCells(wrapper, "price")).toEqual(["-", "-"]);
  });

  it("分红行：现金腿金额与到账账户上屏，无数量/单价/手续费", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    // 分红行（第 5 行）：到账账户 = 账户端「银行」
    expect(colCells(wrapper, "amount")[4]).toBe(formatAmount(30000));
    expect(colCells(wrapper, "account")[4]).toBe("银行");
    expect(colCells(wrapper, "quantity")[4]).toBe("-");
    expect(colCells(wrapper, "price")[4]).toBe("-");
    expect(colCells(wrapper, "fee")[4]).toBe("-");
  });

  it("「共 N 条」上屏且随行集总数（服务端 total）", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    expect(wrapper.find('[data-testid="ledger-total"]').text()).toBe("共 5 条");
  });
});

/**
 * 翻页与类型筛选（双断言：请求参数 + 渲染效果，spec 测试决策）：
 * 服务端 offset 分页（页码/页大小入参）+ 类型子集多选（空集合 ≡ 不过滤）。
 * 删除分页或筛选接线即对应断言变红。
 */
describe("明细页签翻页与类型筛选（issue #1779）", () => {
  it("打开即以默认分页请求（第 1 页、默认页大小），无类型维度（空 ≡ 不过滤）", async () => {
    ledgerDb = manyBuyRows;
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    expect(lastLedgerFilter()).toEqual({ page: 1, page_size: 20 });
    expect(selectedKinds(wrapper)).toEqual([]);
    // 渲染效果：第 1 页 20 行上屏
    expect(colCells(wrapper, "date")).toHaveLength(20);
  });

  it("翻到第 2 页：请求携带 page=2，第 2 页 5 行上屏", async () => {
    ledgerDb = manyBuyRows;
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    const callsBefore = ledgerCalls().length;
    tablePagination(wrapper).onChange(2);
    await flushPromises();
    expect(ledgerCalls().length).toBe(callsBefore + 1);
    expect(lastLedgerFilter()).toEqual({ page: 2, page_size: 20 });
    // 渲染效果：第 2 页 5 行（21 ~ 25 号）
    expect(colCells(wrapper, "date")).toHaveLength(5);
    expect(colCells(wrapper, "date")[0]).toBe("2026-02-21");
  });

  it("页大小切 50：请求携带 page_size=50 并翻回第 1 页", async () => {
    ledgerDb = manyBuyRows;
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    tablePagination(wrapper).onChange(2);
    await flushPromises();
    const callsBefore = ledgerCalls().length;
    tablePagination(wrapper).onUpdatePageSize(50);
    await flushPromises();
    expect(ledgerCalls().length).toBe(callsBefore + 1);
    expect(lastLedgerFilter()).toEqual({ page: 1, page_size: 50 });
    // 渲染效果：25 行全量在一页内
    expect(colCells(wrapper, "date")).toHaveLength(25);
  });

  it("类型筛选选「买入」：请求携带 kinds=[buy] 且翻回第 1 页，渲染只剩买入行", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    const callsBefore = ledgerCalls().length;
    await selectKinds(wrapper, ["buy"]);
    expect(ledgerCalls().length).toBe(callsBefore + 1);
    expect(lastLedgerFilter()).toEqual({ page: 1, page_size: 20, kinds: ["buy"] });
    // 渲染效果：只剩买入行（受控值投影按闭集顺序）
    expect(selectedKinds(wrapper)).toEqual(["buy"]);
    expect(colCells(wrapper, "kind")).toEqual(["买入"]);
  });

  it("类型多选取或：买 + 分红两 kind 并集渲染，筛选值按闭集顺序投影", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await selectKinds(wrapper, ["dividend", "buy"]);
    expect(lastLedgerFilter()).toMatchObject({ kinds: ["dividend", "buy"], page: 1 });
    expect(colCells(wrapper, "kind")).toEqual(["买入", "分红"]);
    expect(selectedKinds(wrapper)).toEqual(["buy", "dividend"]);
  });

  it("清除类型筛选（空集合）：请求不再携带 kinds，渲染回全量行集", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await selectKinds(wrapper, ["buy"]);
    expect(colCells(wrapper, "kind")).toEqual(["买入"]);
    await selectKinds(wrapper, []);
    expect(lastLedgerFilter()).toEqual({ page: 1, page_size: 20 });
    expect(colCells(wrapper, "kind")).toEqual(["买入", "卖出", "转换", "份额调整", "分红"]);
  });

  it("回访首拉遇缩库自愈：保留页码已是空页 → 回退一页重拉，不渲染空页", async () => {
    ledgerDb = manyBuyRows;
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    tablePagination(wrapper).onChange(2);
    await flushPromises();
    expect(colCells(wrapper, "date")).toHaveLength(5);
    // 切走页签期间行集缩到一页；切回明细页签的首拉仍以保留页码（第 2 页）请求
    await clickTab(wrapper, "持仓");
    ledgerDb = manyBuyRows.slice(0, 5);
    const callsBefore = ledgerCalls().length;
    await clickTab(wrapper, "明细");
    await flushPromises();
    // 空页响应不落地（不渲染空页），页码写回保留态第 1 页并以第 1 页重拉
    expect(useInvestmentsSessionStore().detailPage).toBe(1);
    expect(ledgerCalls().length).toBe(callsBefore + 2);
    expect(lastLedgerFilter()).toMatchObject({ page: 1, page_size: 20 });
    expect(colCells(wrapper, "date")).toHaveLength(5);
    expect(colCells(wrapper, "date")[0]).toBe("2026-02-01");
  });
});

/**
 * 空态与加载态（用户可观察）：无数据空态 / 筛选无匹配空态分型；加载中 loading
 * 置位、空态节点不渲染。
 */
describe("明细页签空态与加载态（issue #1779）", () => {
  it("无数据：默认空态「暂无投资交易」", async () => {
    ledgerDb = [];
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    expect(wrapper.find('[data-testid="ledger-empty"]').text()).toContain("暂无投资交易");
  });

  it("筛选无匹配：空态切换为「当前筛选无匹配交易」", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    // 先筛「卖出」（有匹配），再收窄行集为仅买入并重选「卖出」→ 筛选无匹配
    await selectKinds(wrapper, ["sell"]);
    expect(colCells(wrapper, "kind")).toEqual(["卖出"]);
    ledgerDb = [buyRow];
    await selectKinds(wrapper, []);
    await selectKinds(wrapper, ["sell"]);
    expect(wrapper.find('[data-testid="ledger-empty"]').text()).toContain("当前筛选无匹配交易");
  });

  it("加载中：表格 loading 置位；完成后行集上屏", async () => {
    let release!: (v: { items: InvestmentTransactionRow[]; total: number }) => void;
    wireInvokeSeam({
      defaults: LEDGER_DEFAULTS,
      overrides: {
        list_investment_transactions: () =>
          new Promise((resolve) => {
            release = resolve;
          }),
      },
    });
    const wrapper = mountView();
    await flushPromises();
    await clickTab(wrapper, "明细");
    await flushPromises();
    // 在途：loading 置位、空态节点不渲染（加载与空态不同屏）
    expect(ledgerTable(wrapper).props("loading")).toBe(true);
    expect(wrapper.find('[data-testid="ledger-empty"]').exists()).toBe(false);
    release({ items: [buyRow], total: 1 });
    await flushPromises();
    expect(ledgerTable(wrapper).props("loading")).toBe(false);
    expect(colCells(wrapper, "kind")).toEqual(["买入"]);
  });
});

/**
 * 会话内保留（ADR-0094 / ADR-0135）：类型筛选与页码随投资页会话 store 保留——
 * 切走页签再切回以保留态重拉（数据现拉非快照）；删除 store 接线即红。
 */
describe("明细页签会话内保留（issue #1779）", () => {
  it("切走再切回：类型筛选与页码恢复，以保留态重拉", async () => {
    ledgerDb = manyBuyRows;
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await selectKinds(wrapper, ["buy"]);
    tablePagination(wrapper).onChange(2);
    await flushPromises();
    expect(lastLedgerFilter()).toMatchObject({ kinds: ["buy"], page: 2 });
    // 切到持仓再回明细：页签重挂，筛选与页码经会话 store 恢复（任一丢失即红）
    await clickTab(wrapper, "持仓");
    await clickTab(wrapper, "明细");
    await flushPromises();
    expect(lastLedgerFilter()).toMatchObject({ kinds: ["buy"], page: 2 });
    expect(selectedKinds(wrapper)).toEqual(["buy"]);
    // 恢复的是选择不是快照：第 2 页 5 行买入照常现拉上屏
    expect(colCells(wrapper, "kind")).toHaveLength(5);
    expect(colCells(wrapper, "kind")[0]).toBe("买入");
  });

  it("新 pinia 表达冷启动：明细筛选与页码回默认", () => {
    const store = useInvestmentsSessionStore();
    store.setActiveTab("ledger");
    store.setDetailKinds(["buy"]);
    store.setDetailPage(3);
    setActivePinia(createPinia());
    const cold = useInvestmentsSessionStore();
    expect(cold.activeTab).toBe("overview");
    expect(cold.detailKinds).toBeNull();
    expect(cold.detailPage).toBe(1);
  });
});

/**
 * 移动档（ADR-0135 决策 8）：明细页签不做卡片双渲染——同一表格在移动档照常
 * 渲染全部列，scroll-x = 固定列宽总和吸收窄窗口（媒体查询测试接缝换档，
 * issue #841）。列被压碎或 scroll-x 缺失即红。
 */
describe("明细页签移动档横向滚动（issue #1779）", () => {
  it("移动档：同一表格渲染全部列，scroll-x = 固定列宽总和（列不压碎）", async () => {
    setFakeMedia({ width: 400, hover: "hover", pointer: "fine" });
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    const table = ledgerTable(wrapper);
    expect(table.exists()).toBe(true);
    // 全部列头在场（移动档不丢列）
    const headers = table.findAll("th").map((th) => th.text());
    expect(headers).toEqual([
      "日期",
      "类型",
      "标的",
      "数量",
      "金额",
      "单价",
      "手续费",
      "账户",
      "出资账户",
    ]);
    // scroll-x = 固定列宽总和（标的列弹性 minWidth 不计入）
    const columns = table.props("columns") as unknown as Array<{ width?: number }>;
    const fixedSum = columns.reduce(
      (sum, c) => sum + (typeof c.width === "number" ? c.width : 0),
      0,
    );
    expect(fixedSum).toBeGreaterThan(0);
    expect(table.props("scrollX")).toBe(fixedSum);
  });
});

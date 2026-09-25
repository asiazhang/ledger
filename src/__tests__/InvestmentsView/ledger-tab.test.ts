import { afterEach, describe, it, expect, vi, beforeEach } from "vitest";
import { lastInvokeArgs, mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { defineComponent, h, reactive } from "vue";
import { createPinia, setActivePinia } from "pinia";
import { NButton, NDataTable, NModal, NSelect, NTag } from "naive-ui";
import InvestmentsView from "@/views/InvestmentsView.vue";
import InvestmentLedgerTab from "@/investment/InvestmentLedgerTab.vue";
import TransactionForm from "@/transaction/TransactionForm.vue";
import { KIND_TAG_TYPE } from "@/transaction/transaction-columns";
import InvestmentForm from "@/investment/InvestmentForm.vue";
import { useInvestmentsSessionStore } from "@/investment/investments-session";
import { useAppStore } from "@/stores/app";
import { formatAmount, formatPrice, formatQuantity } from "@ledger/money";
import { kindSemanticColor } from "@ledger/theme/semantic-colors";
import { clickTab } from "@ledger/test-support/dom";
import { componentVm } from "@ledger/test-support/component-vm";
import { mountWithDialog } from "@ledger/test-support/mount";
import { setFakeMedia } from "@ledger/test-support/media-mock";
import AppSelect from "@ledger/ui-kit/AppSelect.vue";
import PinyinSelect from "@ledger/ui-kit/PinyinSelect.vue";
import { useWindowGuard } from "@/composables/useWindowGuard";
import { clearViewResets } from "@/composables/viewResetRegistry";
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
const mockRoute = reactive({ query: {} as Record<string, string> });
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

/** 与后端 ledger_tab 口径一致：四维过滤（kind 子集取或、涉及账户两端、标的含 convert 两腿、
 *  日期双端有界）+ offset 分页，total 恒返回。 */
function listInvestmentTransactions(args?: { filter?: Record<string, unknown> }) {
  const filter = args?.filter ?? {};
  const scoped = ledgerDb.filter((r) => {
    if (Array.isArray(filter.kinds) && !filter.kinds.includes(r.kind)) return false;
    // 涉及账户：账户端 ∪ 出资端（ADR-0096 出资账户）
    if (typeof filter.account_id === "string") {
      if (r.account_id !== filter.account_id && r.funding_account_id !== filter.account_id) {
        return false;
      }
    }
    // 标的：转出腿或 convert 转入腿任一命中
    if (typeof filter.instrument_id === "string") {
      if (
        r.instrument_id !== filter.instrument_id &&
        r.convert?.to_instrument_id !== filter.instrument_id
      ) {
        return false;
      }
    }
    if (typeof filter.from === "string" && r.date < filter.from) return false;
    if (typeof filter.to === "string" && r.date > filter.to) return false;
    return true;
  });
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
  // 累计收益·折本位币单值（issue #1797）：空账本为 0 单值
  cumulative_pnl_native_total: { total_cents: 0, native_currency: "CNY" },
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
  // 数据期间边界（QuickTimeRange 钳制输入，#1807）：「今天」= 2026-02-10 时各档边界覆盖夹具期间
  report_date_range: { min_date: "2025-12-15", max_date: "2026-03-01" },
};

beforeEach(async () => {
  mockRoute.query = {};
  // 复位回调注册表清场：上一用例未卸载的视图注册不泄漏进本用例的 ESC 判定
  clearViewResets();
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

/**
 * 筛选行下拉与日期（受控装配缝直发回传，与用户操作同一路径）：按 testid 定位
 * 筛选行控件（PinyinSelect / AppDatePicker），直发 update 事件。
 */
function findFilterSelect(wrapper: VueWrapper, testid: string) {
  const select = ledgerPane(wrapper)
    .findAllComponents(PinyinSelect)
    .find((c) => c.attributes("data-testid") === testid);
  expect(select, `应存在筛选控件 ${testid}`).toBeTruthy();
  return select!;
}

async function selectFilter(wrapper: VueWrapper, testid: string, value: string | string[] | null) {
  componentVm(findFilterSelect(wrapper, testid)).$emit("update:value", value);
  await flushPromises();
}

function filterValue(wrapper: VueWrapper, testid: string): unknown {
  return findFilterSelect(wrapper, testid).findComponent(NSelect).props("value");
}

/**
 * 冻结「今天」（只伪造 Date、保留真实定时器以免 flushPromises 停摆）：芯片边界
 * 断言确定化（SearchView 时间维度测试同款日期 2026-02-10）。
 */
function freezeToday() {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date(2026, 1, 10, 12, 0, 0));
}

afterEach(() => vi.useRealTimers());

/** 时间芯片按文案定位（闭集文案唯一：全部/当月/当季/当年/去年，SearchView 同款）。 */
function chip(wrapper: VueWrapper, label: string) {
  return wrapper.findAllComponents(NButton).find((b) => b.text().trim() === label)!;
}

async function clickChip(wrapper: VueWrapper, label: string) {
  await chip(wrapper, label).trigger("click");
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
      // 操作列（issue #1781）：行「⋯」菜单入口（触控轴行菜单的唯一入口，无卡片双渲染）
      "操作",
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

/**
 * 明细页签手动筛选三维 + 标的下钻只读入口（issue #1780 / ADR-0135 决策 3 及其修订注记）：
 * 账户（涉及账户语义——账户端 ∪ 出资端）与日期（时间范围快捷选择）与类型组合过滤；
 * 标的维度无手动控件（仅 ?instrument= 深链/会话落账，convert 两腿任一命中即算）；
 * 任一维度实际变化翻页归零（store 内化）。双断言：请求参数 + 渲染效果；删除任一
 * 维度接线即对应断言变红。
 */
describe("明细页签账户/标的/日期筛选（issue #1780）", () => {
  it("账户下拉候选收窄为投资类账户：非投资类账户（出资/到账账户）不进选项面（#1828）", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    // 候选 = 投资账户谓词单点收口（参考 store investmentAccounts，与持仓/盈亏/录入表单同源）：
    // 券商户（investment）在场，银行（bank，出资/到账账户）不进选项面
    const options = findFilterSelect(wrapper, "ledger-account-filter")
      .findComponent(NSelect)
      .props("options") as Array<{ label: string; value: string }>;
    expect(options).toEqual([{ label: "券商户", value: "acc-1" }]);
  });

  it("账户筛选：请求携带 account_id 且翻回第 1 页，涉及账户两端命中（账户端 + 出资端）", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    const callsBefore = ledgerCalls().length;
    await selectFilter(wrapper, "ledger-account-filter", "acc-2");
    expect(ledgerCalls().length).toBe(callsBefore + 1);
    expect(lastLedgerFilter()).toEqual({ page: 1, page_size: 20, account_id: "acc-2" });
    // 渲染效果：涉及账户两端命中——分红行（账户端到账）与买入行（出资账户直扣）
    expect(filterValue(wrapper, "ledger-account-filter")).toBe("acc-2");
    expect(colCells(wrapper, "kind")).toEqual(["买入", "分红"]);
  });

  it("标的筛选控件已退役（仅下钻只读入口）：无标的下拉，会话维度仍参与过滤", async () => {
    const store = useInvestmentsSessionStore();
    store.setDetailInstrument("inst-msft");
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    // 手动控件不存在（ADR-0135 修订注记：降级为仅 URL 下钻只读入口）
    expect(ledgerPane(wrapper).findAll('[data-testid="ledger-instrument-filter"]')).toHaveLength(0);
    // 隐藏维度照常参与请求与行过滤——inst-msft 仅出现在转换行转入腿（两腿口径）
    expect(lastLedgerFilter()).toMatchObject({ instrument_id: "inst-msft", page: 1 });
    expect(colCells(wrapper, "kind")).toEqual(["转换"]);
  });

  it("日期筛选：点「当月」写入双端有界快照（含边界），渲染只剩区间内行", async () => {
    freezeToday();
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await clickChip(wrapper, "当月");
    expect(lastLedgerFilter()).toMatchObject({ from: "2026-02-01", to: "2026-02-28", page: 1 });
    // 夹具仅分红行（2026-02-28）落在当月区间内
    expect(colCells(wrapper, "date")).toEqual(["2026-02-28"]);
  });

  it("日期清除：点「全部」清回双空默认态（请求不再携带 from/to）", async () => {
    freezeToday();
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await clickChip(wrapper, "当月");
    await clickChip(wrapper, "全部");
    expect(lastLedgerFilter()).toEqual({ page: 1, page_size: 20 });
    expect(colCells(wrapper, "kind")).toHaveLength(5);
  });

  it("组合过滤：类型 × 账户 AND 组合，翻页归零随最后一维变化", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await selectKinds(wrapper, ["dividend"]);
    await selectFilter(wrapper, "ledger-account-filter", "acc-2");
    expect(lastLedgerFilter()).toEqual({
      page: 1,
      page_size: 20,
      kinds: ["dividend"],
      account_id: "acc-2",
    });
    expect(colCells(wrapper, "kind")).toEqual(["分红"]);
  });

  it("筛选无匹配：空态切换为「当前筛选无匹配交易」", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await selectKinds(wrapper, ["sell"]);
    await selectFilter(wrapper, "ledger-account-filter", "acc-2");
    // 账户 acc-2（命中买入/分红行）× 类型 sell（仅卖出行，账户 acc-1）无交集行
    expect(wrapper.find('[data-testid="ledger-empty"]').text()).toContain("当前筛选无匹配交易");
  });

  it("会话内保留：账户/标的/日期三维切走页签再回来恢复并以保留态重拉", async () => {
    freezeToday();
    // 标的维度的写入方是深链（一次性消费），此处以 store 直写模拟其保留态
    useInvestmentsSessionStore().setDetailInstrument("inst-1");
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await selectFilter(wrapper, "ledger-account-filter", "acc-2");
    await clickChip(wrapper, "当年");
    const last = lastLedgerFilter();
    expect(last).toMatchObject({
      account_id: "acc-2",
      instrument_id: "inst-1",
      from: "2026-01-01",
      to: "2026-12-31",
    });
    await clickTab(wrapper, "持仓");
    await clickTab(wrapper, "明细");
    await flushPromises();
    // 恢复以保留态请求（任一维度丢失即红）
    expect(lastLedgerFilter()).toMatchObject({
      account_id: "acc-2",
      instrument_id: "inst-1",
      from: "2026-01-01",
      to: "2026-12-31",
    });
    expect(filterValue(wrapper, "ledger-account-filter")).toBe("acc-2");
  });
});

/**
 * 明细页签清除筛选按钮（#1807，主列表同款判定）：任一明细维度激活（含深链带入的
 * 标的隐藏维度）即可用；点击清明细四维 + 翻页归零，不切页签、不动页大小——明细面
 * 与 ESC 复位同源，页签回概览仍是 ESC 复位专属出口。删除按钮接线即红。
 */
describe("明细页签清除筛选按钮（#1807）", () => {
  function clearButton(wrapper: VueWrapper) {
    const btn = ledgerPane(wrapper).find('[data-testid="ledger-clear-filters"]');
    expect(btn.exists(), "明细页签应有「清除筛选」按钮").toBe(true);
    return btn!;
  }

  it("默认态禁用；任一维度激活（含标的隐藏维度）点亮，点击清明细四维 + 翻页归零且不切页签", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    expect(clearButton(wrapper).attributes("disabled")).toBeDefined();
    await selectKinds(wrapper, ["buy"]);
    await selectFilter(wrapper, "ledger-account-filter", "acc-2");
    // 深链带入的标的隐藏维度同样点亮按钮
    const store = useInvestmentsSessionStore();
    store.setDetailInstrument("inst-1");
    store.setDetailPage(2);
    await flushPromises();
    expect(clearButton(wrapper).attributes("disabled")).toBeUndefined();
    await clearButton(wrapper).trigger("click");
    await flushPromises();
    expect(store.detailKinds).toBeNull();
    expect(store.detailAccountId).toBeNull();
    expect(store.detailInstrumentId).toBeNull();
    expect(store.detailDateFrom).toBeNull();
    expect(store.detailDateTo).toBeNull();
    expect(store.detailPage).toBe(1);
    expect(store.activeTab).toBe("ledger");
    expect(lastLedgerFilter()).toEqual({ page: 1, page_size: 20 });
  });
});

/**
 * 呈现面对齐主列表（#1807 / ADR-0135 修订注记）：金额列语义色（kindSemanticColor）
 * 与类型列标签色（KIND_TAG_TYPE）与主列表同源同件。删除任一着色接线即红。
 */
describe("明细页签金额/类型着色与主列表同源（#1807）", () => {
  it("金额列按 kind 语义色着色（AmountCell），类型列 NTag 标签色同 KIND_TAG_TYPE", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    // 金额单元格：首行买入行，色值与主列表同源函数计算结果一致
    const amountCell = ledgerPane(wrapper).find('td[data-col-key="amount"] .amount-cell');
    expect(amountCell.exists()).toBe(true);
    // Vue 会把 hex 色值序列化为 rgb() 形态，比较前换算（色值本体与主列表同源）
    const color = kindSemanticColor("buy", useAppStore().theme);
    const [r, g, b] = [1, 3, 5].map((i) => parseInt(color.slice(i, i + 2), 16));
    expect(amountCell.attributes("style")).toContain(`rgb(${r}, ${g}, ${b})`);
    // 类型列：行序 date 倒序 = buy/sell/convert/split/dividend，标签色与主列表映射同源
    const rowKinds = ["buy", "sell", "convert", "split", "dividend"] as const;
    const kindCells = ledgerPane(wrapper).findAll('td[data-col-key="kind"]');
    const types = kindCells.map((c) => c.findComponent(NTag).props("type"));
    expect(types).toEqual(rowKinds.map((k) => KIND_TAG_TYPE[k]));
  });
});

/**
 * 明细页签 ESC 复位扩展（issue #1780 / ADR-0094 决策 4）：无弹层 ESC 复位回调已在
 * issue #1192 接线（删除接线即既有测试变红）；本票断言复位把明细筛选新增三维一并
 * 清零——复位即清除保留态本身。
 */
describe("明细页签 ESC 复位覆盖新增三维（issue #1780）", () => {
  function mountGuardHost() {
    const Host = defineComponent({
      setup() {
        useWindowGuard();
        return () => h("div");
      },
    });
    return mountWithDialog(Host);
  }

  function fireEscape() {
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
  }

  it("无弹层 ESC：账户/标的/日期三维清零、翻页归零、页签回概览", async () => {
    const guard = mountGuardHost();
    await flushPromises();
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await selectKinds(wrapper, ["buy"]);
    await selectFilter(wrapper, "ledger-account-filter", "acc-2");
    await clickChip(wrapper, "当年");
    useInvestmentsSessionStore().setDetailPage(2);
    await flushPromises();

    fireEscape();
    await flushPromises();
    const store = useInvestmentsSessionStore();
    expect(store.activeTab).toBe("overview");
    expect(store.detailKinds).toBeNull();
    expect(store.detailAccountId).toBeNull();
    expect(store.detailInstrumentId).toBeNull();
    expect(store.detailDateFrom).toBeNull();
    expect(store.detailDateTo).toBeNull();
    expect(store.detailPage).toBe(1);
    // 复位即清除保留态本身：回到明细页签是默认全量行集
    await openLedgerTab(wrapper);
    expect(lastLedgerFilter()).toEqual({ page: 1, page_size: 20 });
    expect(colCells(wrapper, "kind")).toHaveLength(5);
    wrapper.unmount();
    guard.unmount();
  });
});

/**
 * 深链落点（issue #1780 / ADR-0135 决策 6）：投资页 URL 参数表新增 `tab=detail`
 * （值即明细页签键）+ `account` / `instrument` 过滤参数——一次性消费、URL 只读
 * 不写回、参数在场永远赢（覆盖保留态对应维度）。删除消费接线即变红。
 */
describe("明细页签深链落点（issue #1780）", () => {
  it("?tab=detail：落明细页签并以默认筛选请求；URL 不写回", async () => {
    mockRoute.query = { tab: "detail" };
    const wrapper = mountView();
    await flushPromises();
    expect(useInvestmentsSessionStore().activeTab).toBe("ledger");
    expect(wrapper.findAll(".n-tabs-tab--active").map((el) => el.text())).toEqual(["明细"]);
    // 已在明细页签的默认态请求（一次性消费即生效，页签重挂不重放）
    expect(lastLedgerFilter()).toEqual({ page: 1, page_size: 20 });
    expect(pushMock).not.toHaveBeenCalled();
  });

  it("?tab=detail&account=：参数在场永远赢，覆盖保留态对应维度", async () => {
    const store = useInvestmentsSessionStore();
    store.setDetailAccount("acc-1");
    mockRoute.query = { tab: "detail", account: "acc-2" };
    const wrapper = mountView();
    await flushPromises();
    expect(store.activeTab).toBe("ledger");
    expect(lastLedgerFilter()).toMatchObject({ account_id: "acc-2", page: 1 });
    expect(colCells(wrapper, "kind")).toEqual(["买入", "分红"]);
    expect(pushMock).not.toHaveBeenCalled();
  });

  it("?tab=detail&account=&instrument=：账户 + 标的组合落账（持仓行下钻载荷形态，ADR-0135 决策 6）", async () => {
    mockRoute.query = { tab: "detail", account: "acc-2", instrument: "inst-1" };
    const wrapper = mountView();
    await flushPromises();
    expect(useInvestmentsSessionStore().activeTab).toBe("ledger");
    // 落点渲染双断言（ADR-0087）：请求携带双参 + 列表按账户∩标的过滤渲染
    expect(lastLedgerFilter()).toMatchObject({
      account_id: "acc-2",
      instrument_id: "inst-1",
      page: 1,
    });
    expect(colCells(wrapper, "kind")).toEqual(["买入", "分红"]);
    wrapper.unmount();
  });

  it("恒可达：未命中标的字典的 id 照常携带请求（空态可见，不产生落空跳转，ADR-0107 决策 5）", async () => {
    mockRoute.query = { tab: "detail", instrument: "inst-no-such" };
    const wrapper = mountView();
    await flushPromises();
    expect(useInvestmentsSessionStore().activeTab).toBe("ledger");
    expect(lastLedgerFilter()).toMatchObject({ instrument_id: "inst-no-such" });
    expect(colCells(wrapper, "kind")).toEqual([]);
    wrapper.unmount();
  });

  it("?tab=detail&instrument=：标的维度落账（convert 两腿口径）", async () => {
    mockRoute.query = { tab: "detail", instrument: "inst-msft" };
    const wrapper = mountView();
    await flushPromises();
    expect(lastLedgerFilter()).toMatchObject({ instrument_id: "inst-msft", page: 1 });
    expect(colCells(wrapper, "kind")).toEqual(["转换"]);
  });

  it("一次性消费：同载荷不重放（消费后手动清除不因 URL 在场而复活）", async () => {
    mockRoute.query = { tab: "detail", account: "acc-2" };
    const wrapper = mountView();
    await flushPromises();
    expect(useInvestmentsSessionStore().detailAccountId).toBe("acc-2");
    // 用户手动清除账户筛选
    await openLedgerTab(wrapper);
    await selectFilter(wrapper, "ledger-account-filter", null);
    expect(useInvestmentsSessionStore().detailAccountId).toBeNull();
    // 同载荷再次在 query 在场（同一 URL 不变）不重放：筛选保持清除
    mockRoute.query = { tab: "detail", account: "acc-2" };
    await flushPromises();
    expect(useInvestmentsSessionStore().detailAccountId).toBeNull();
    wrapper.unmount();
  });

  it("新意图：在途新载荷（换标的深链）重新消费并覆盖", async () => {
    mockRoute.query = { tab: "detail", account: "acc-2" };
    const wrapper = mountView();
    await flushPromises();
    // 会话内在途换一个标的深链（不同载荷）→ 重新消费
    mockRoute.query = { tab: "detail", account: "acc-2", instrument: "inst-msft" };
    await flushPromises();
    const store = useInvestmentsSessionStore();
    expect(store.detailInstrumentId).toBe("inst-msft");
    expect(lastLedgerFilter()).toMatchObject({ instrument_id: "inst-msft", page: 1 });
    wrapper.unmount();
  });

  it("参数表之外的 tab 值不消费（本票仅新增 detail 一个入口）", async () => {
    mockRoute.query = { tab: "holdings" };
    const wrapper = mountView();
    await flushPromises();
    expect(useInvestmentsSessionStore().activeTab).toBe("overview");
    wrapper.unmount();
  });
});

/**
 * 明细页签头部记买入/卖出（ADR-0135 决策 5 / issue #1782）：创建入口随迁——入口落页签
 * 头部，提交走既有创建编排（useTransactionModalState 记一笔意图）与 TransactionInput
 * 装配接缝（TransactionForm → InvestmentForm → buildTradeInput），成功后明细列表刷新
 * 可见新行（翻回第 1 页，date 倒序新记录最可能可见）。删除任一接线即红，断言对准
 * 用户可观察结果（弹窗标题/表单类型/create_transaction 载荷/列表重拉页码，ADR-0087）。
 */
describe("明细页签头部记买入/卖出（issue #1782）", () => {
  /** 明细页签内按可见文案找头部入口按钮（页签内查找，避开弹窗提交按钮同名文案）。 */
  function createEntry(wrapper: VueWrapper, label: string) {
    const btn = ledgerPane(wrapper)
      .findAll("button")
      .find((b) => b.text().includes(label));
    expect(btn, `明细页签头部应有「${label}」入口`).toBeDefined();
    return btn!;
  }

  /** 打开弹窗后展示中的记一笔弹窗（意图非空即显示派生 show）。 */
  function shownModal(wrapper: VueWrapper) {
    return wrapper.findAllComponents(NModal).find((m) => m.props("show") === true);
  }

  it.each([
    ["记买入", "买入", "buy"],
    ["记卖出", "卖出", "sell"],
  ] as const)(
    "头部「%s」入口：点开「记一笔 · %s」弹窗，复用创建编排与交易表单装配接缝",
    async (label, kindLabel, kind) => {
      const wrapper = mountView();
      await flushPromises();
      await openLedgerTab(wrapper);
      await createEntry(wrapper, label).trigger("click");
      await flushPromises();
      const modal = shownModal(wrapper);
      expect(modal, "记一笔弹窗应已打开").toBeDefined();
      expect(modal!.props("title")).toBe(`记一笔 · ${kindLabel}`);
      const form = modal!.findComponent(TransactionForm);
      expect(form.exists()).toBe(true);
      expect(form.props("kind")).toBe(kind);
      // 装配接缝复用：买入/卖出按标的形式分派 InvestmentForm（基金金额权威/非基金单价权威）
      expect(form.findComponent(InvestmentForm).exists()).toBe(true);
    },
  );

  it("真实提交链路：填表提交 → create_transaction（TransactionInput 装配）→ 弹窗关闭 + 明细列表刷新翻回第 1 页", async () => {
    ledgerDb = manyBuyRows;
    // 表单依赖布线：投资账户字典（同 beforeEach 覆写）+ 标的字典（领域命令自接）+ 目标进度空集
    wireInvokeSeam({
      defaults: LEDGER_DEFAULTS,
      overrides: {
        list_accounts: [
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
        ],
        list_investment_transactions: listInvestmentTransactions,
        list_instruments: { items: [], total: 0 },
        savings_goal_progress: Promise.resolve([]),
        create_transaction: Promise.resolve("new-investment-id"),
      },
    });
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    // 先翻到第 2 页再记一笔：成功后应翻回第 1 页，新记录可见
    tablePagination(wrapper).onChange(2);
    await flushPromises();
    await createEntry(wrapper, "记买入").trigger("click");
    await flushPromises();
    const inv = wrapper.findComponent(TransactionForm).findComponent(InvestmentForm);
    const selects = inv.findAllComponents(NSelect);
    selects[1].vm.$emit("update:value", "acc-1");
    // 内层 NSelect 序（0=币种 1=投资账户 2=出资账户 3=标的，InvestmentForm 表单行序）
    selects[3].vm.$emit("update:value", "ins-1");
    await flushPromises();
    // 数量/单价（placeholder 定位，股票形态权威输入 = 数量 + 单价）
    await inv
      .findAll("input")
      .find((i) => i.attributes("placeholder") === "数量")!
      .setValue("100");
    await inv
      .findAll("input")
      .find((i) => i.attributes("placeholder") === "单价")!
      .setValue("10");
    const callsBefore = ledgerCalls().length;
    await inv
      .findAll("button")
      .find((b) => b.text().includes("记买入"))!
      .trigger("click");
    await flushPromises();
    // 后端收到正确账目（非基金形态：数量 × 单价权威，amount_cents 占位由后端重算）
    const createCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "create_transaction");
    expect(createCalls).toHaveLength(1);
    const [, createArgs] = createCalls[0] as [string, { input: Record<string, unknown> }];
    expect(createArgs.input).toMatchObject({
      kind: "buy",
      account_id: "acc-1",
      instrument_id: "ins-1",
      quantity: 100,
      price_cents: 100000,
      amount_cents: 0,
    });
    // 弹窗关闭 + 明细列表刷新（翻回第 1 页重拉）
    expect(shownModal(wrapper)).toBeUndefined();
    expect(ledgerCalls().length).toBe(callsBefore + 1);
    expect(lastLedgerFilter()).toMatchObject({ page: 1 });
  });

  it("仅关闭弹窗（不提交）不触发明细列表刷新", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await createEntry(wrapper, "记卖出").trigger("click");
    await flushPromises();
    const callsBefore = ledgerCalls().length;
    shownModal(wrapper)!.vm.$emit("update:show", false);
    await flushPromises();
    expect(shownModal(wrapper)).toBeUndefined();
    expect(ledgerCalls().length).toBe(callsBefore);
  });

  it("投资页不设裸键（ADR-0135 决策 5）：裸键 a/z/i/b/s 均不开创建弹窗（记买入/卖出仅头部入口）", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    for (const key of ["a", "z", "i", "b", "s"]) {
      window.dispatchEvent(new KeyboardEvent("keydown", { key, bubbles: true }));
      await flushPromises();
    }
    expect(shownModal(wrapper)).toBeUndefined();
  });
});

import { describe, it, expect, vi, beforeEach } from "vitest";
import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { NDataTable, NDropdown, NInput, NModal } from "naive-ui";
import { lastInvokeArgs, mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import InvestmentsView from "@/views/InvestmentsView.vue";
import InvestmentLedgerTab from "@/investment/InvestmentLedgerTab.vue";
import ConvertDetail from "@/investment/ConvertDetail.vue";
import SplitDetail from "@/investment/SplitDetail.vue";
import DividendDetail from "@/investment/DividendDetail.vue";
import OrderDetail from "@/transaction/OrderDetail.vue";
import InvestmentForm from "@/investment/InvestmentForm.vue";
import {
  clickTab,
  clickDialogButton,
  dialogText,
  visibleModalText,
} from "@ledger/test-support/dom";
import { fireProp } from "@ledger/test-support/component-vm";
import { mountWithDialog } from "@ledger/test-support/mount";
import { messageCalls } from "@ledger/test-support/message-mock";
import { registerToastSink } from "@ledger/loadable";
import { makeFakeSink } from "../factories";
import { makeInvestmentLedgerRow, makeInvestmentOverview, makeMwrSummary } from "../factories";
import type {
  InvestmentTransactionRow,
  TransactionConvert,
  TransactionSplit,
  TransactionTrade,
} from "@ledger/types";

vi.mock("vue-chartjs", async () => {
  const { LineChartStub } = await import("../line-chart-stub");
  return { Line: LineChartStub };
});

vi.mock("@/investment/usePricesChanged", async () => {
  const { capturePricesChangedHandler } = await import("../prices-changed-mock");
  return {
    usePricesChanged: (cb: () => void) => capturePricesChangedHandler(cb),
  };
});

const pushMock = vi.fn();
const mockRoute = { query: {} as Record<string, string> };
vi.mock("vue-router", () => ({
  useRoute: () => mockRoute,
  useRouter: () => ({ push: pushMock }),
}));

const mountView = () => mountWithDialog(InvestmentsView);

const buyRow = makeInvestmentLedgerRow({
  id: "t-buy",
  kind: "buy",
  date: "2026-03-05",
  amount_cents: 10000,
  amount_native_cents: 10000,
  funding_account_id: "acc-2",
  note: "建仓备注",
  trade: { quantity: 10, price_cents: 100000, fee_cents: 100 },
});

const sellRow = makeInvestmentLedgerRow({
  id: "t-sell",
  kind: "sell",
  date: "2026-03-04",
  amount_cents: 4800,
  trade: { quantity: 4, price_cents: 120000, fee_cents: 50 },
});

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

const splitRow = makeInvestmentLedgerRow({
  id: "t-split",
  kind: "split",
  date: "2026-03-02",
  symbol: "000001",
  instrument_name: "平安银行",
  amount_cents: 0,
  split: { delta_quantity: 2 },
});

const dividendRow = makeInvestmentLedgerRow({
  id: "t-dividend",
  kind: "dividend",
  date: "2026-02-28",
  amount_cents: 30000,
  account_id: "acc-2",
  symbol: "AAPL",
  instrument_name: "苹果",
  note: "年中分红",
});

let ledgerDb: InvestmentTransactionRow[] = [];

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

const LEDGER_DEFAULTS = {
  investment_overview: makeInvestmentOverview(),
  list_holdings: [],
  instrument_price_staleness: { stale_count: 0, threshold_days: 3 },
  cumulative_pnl_summary: [],
  portfolio_value_trend: { currency_code: "CNY", points: [] },
  realized_pnl_summary: { by_year: [], by_account: [] },
  money_weighted_return_summary: makeMwrSummary({ by_instrument: [], by_account: [], total: [] }),
};

const tradeDetail: TransactionTrade = {
  instrument_id: "inst-1",
  symbol: "600000",
  instrument_name: "浦发银行",
  instrument_type: "stock",
  quantity: 10,
  price_cents: 100000,
  fee_cents: 100,
};

const convertDetail: TransactionConvert = {
  out_instrument_id: "inst-aapl",
  out_symbol: "AAPL",
  out_instrument_name: "苹果",
  out_quantity: 2,
  out_amount_cents: 20000,
  in_instrument_id: "inst-msft",
  in_symbol: "MSFT",
  in_instrument_name: "微软",
  in_quantity: 1,
  in_amount_cents: 19000,
  fee_cents: 100,
  carried_cost_cents: 21000,
  currency_code: "CNY",
};

const splitDetail: TransactionSplit = {
  instrument_id: "inst-1",
  symbol: "000001",
  instrument_name: "平安银行",
  quantity: 2,
};

beforeEach(async () => {
  mockRoute.query = {};
  ledgerDb = [buyRow, sellRow, convertRow, splitRow, dividendRow];
  await wireInvokeSeam({
    defaults: LEDGER_DEFAULTS,
    overrides: {
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
      get_transaction_trade: () => Promise.resolve(tradeDetail),
      get_transaction_convert: () => Promise.resolve(convertDetail),
      get_transaction_split: () => Promise.resolve(splitDetail),
    },
    refreshReferenceStores: true,
  }).ready;
});

function ledgerPane(wrapper: VueWrapper) {
  const pane = wrapper.findComponent(InvestmentLedgerTab);
  expect(pane.exists(), "明细页签组件应已挂载（先打开明细页签）").toBe(true);
  return pane;
}

async function openLedgerTab(wrapper: VueWrapper) {
  await clickTab(wrapper, "明细");
  await flushPromises();
}

async function openMenuOnRow(wrapper: VueWrapper, index = 0) {
  await ledgerPane(wrapper).findAll("tbody tr")[index].trigger("contextmenu");
  await flushPromises();
}

function rowMenu(wrapper: VueWrapper) {
  return wrapper
    .findAllComponents(NDropdown)
    .find((d) =>
      ((d.props("options") ?? []) as Array<{ key?: string }>).some(
        (o) => o.key === "delete" || o.key === "detail",
      ),
    )!;
}

function rowMenuKeys(wrapper: VueWrapper) {
  return (rowMenu(wrapper).props("options") as Array<{ key: string }>).map((o) => o.key);
}

async function selectRowMenu(wrapper: VueWrapper, key: string) {
  fireProp(rowMenu(wrapper), "onSelect", key);
  await flushPromises();
}

function modalByTitle(wrapper: VueWrapper, title: string) {
  return wrapper.findAllComponents(NModal).find((m) => m.props("title") === title)!;
}

function ledgerCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === "list_investment_transactions");
}

function deleteCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === "delete_transaction");
}

function withCommandOverride(cmd: string, handler: (args?: Record<string, unknown>) => unknown) {
  const base = mockInvoke.getMockImplementation()!;
  mockInvoke.mockImplementation((c: string, args?: Record<string, unknown>) =>
    c === cmd ? Promise.resolve(handler(args)) : base(c, args),
  );
}

describe("明细页签行菜单与行激活闭集（issue #1781）", () => {
  it.each([
    ["买入", 0, ["edit", "menu-divider", "delete"]],
    ["卖出", 1, ["edit", "menu-divider", "delete"]],
    ["转换", 2, ["detail"]],
    ["份额调整", 3, ["detail"]],
    ["分红", 4, ["detail"]],
  ])("%s 行右键菜单项符合行激活闭集", async (_label, index, expected) => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, index);
    expect(rowMenuKeys(wrapper)).toEqual(expected);
  });

  it("「⋯」操作列与右键同一菜单：点开集合一致（同一 RowContextMenu open 入口）", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    const cols = ledgerPane(wrapper).findComponent(NDataTable).props("columns") as Array<{
      key?: string;
    }>;
    expect(cols[cols.length - 1].key).toBe("actions");
    expect(ledgerPane(wrapper).findAll(".row-actions-btn")).toHaveLength(5);
    await openMenuOnRow(wrapper, 2);
    const rightClickKeys = rowMenuKeys(wrapper);
    await ledgerPane(wrapper).findAll(".row-actions-btn")[2].trigger("click");
    await flushPromises();
    expect(rowMenuKeys(wrapper)).toEqual(rightClickKeys);
  });
});

describe("明细页签编辑 buy/sell（issue #1781）", () => {
  function editModal(wrapper: VueWrapper) {
    return modalByTitle(wrapper, "编辑交易");
  }

  it("buy 行选编辑：先取买卖明细，弹窗打开且投资表单回填标的/数量/备注/出资账户", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "edit");
    expect(lastInvokeArgs("get_transaction_trade")).toEqual({ id: "t-buy" });
    expect(editModal(wrapper).props("show")).toBe(true);
    expect(wrapper.findAllComponents(InvestmentForm)).toHaveLength(1);
    const form = wrapper.findComponent(InvestmentForm);
    expect(form.props("editing")).toMatchObject({ id: "t-buy" });
    expect(form.props("trade")).toMatchObject({ symbol: "600000", quantity: 10 });
    const inputs = form.findAllComponents(NInput);
    expect(inputs[inputs.length - 1].props("value")).toBe("建仓备注");
  });

  it("sell 行选编辑：取数并开窗（同权入口，sell 同走投资表单）", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, 1);
    await selectRowMenu(wrapper, "edit");
    expect(lastInvokeArgs("get_transaction_trade")).toEqual({ id: "t-sell" });
    expect(editModal(wrapper).props("show")).toBe(true);
  });

  it("编辑提交：update_transaction 全字段替换，弹窗关闭且保持当前页重拉", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "edit");
    const form = wrapper.findComponent(InvestmentForm);
    const base = mockInvoke.getMockImplementation()!;
    mockInvoke.mockImplementationOnce((cmd: string, args?: Record<string, unknown>) =>
      cmd === "update_transaction" ? Promise.resolve() : base(cmd, args),
    );
    await form
      .findAll("button")
      .find((b) => b.text().includes("保存修改"))!
      .trigger("click");
    await flushPromises();
    const updateCalls = mockInvoke.mock.calls.filter(([cmd]) => cmd === "update_transaction");
    expect(updateCalls).toHaveLength(1);
    const [, { id }] = updateCalls[0] as unknown as [string, { id: string }];
    expect(id).toBe("t-buy");
    expect(editModal(wrapper).props("show")).toBe(false);
    expect(ledgerCalls().length).toBeGreaterThanOrEqual(2);
  });

  it("取数失败不开窗：买卖明细读取失败 → 错误提示、编辑弹窗不出现", async () => {
    withCommandOverride("get_transaction_trade", () => Promise.reject(new Error("无买卖明细")));
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "edit");
    expect(editModal(wrapper).props("show")).toBe(false);
    expect(messageCalls().some((m) => m.method === "error" && m.text.includes("无买卖明细"))).toBe(
      true,
    );
  });
});

describe("明细页签只读详情 convert/split/dividend（issue #1781）", () => {
  it("convert 行选详情：先取两腿明细，弹窗呈现「A → B」双腿", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, 2);
    await selectRowMenu(wrapper, "detail");
    expect(lastInvokeArgs("get_transaction_convert")).toEqual({ id: "t-convert" });
    const modal = modalByTitle(wrapper, "交易详情");
    expect(modal.props("show")).toBe(true);
    expect(modal.findComponent(ConvertDetail).props("convert")).toMatchObject({
      out_symbol: "AAPL",
      in_symbol: "MSFT",
    });
    expect(visibleModalText()).toContain("AAPL 苹果");
    expect(visibleModalText()).toContain("MSFT 微软");
  });

  it("split 行选详情：先取份额调整明细，弹窗呈现带符号份额变动与账户", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, 3);
    await selectRowMenu(wrapper, "detail");
    expect(lastInvokeArgs("get_transaction_split")).toEqual({ id: "t-split" });
    const modal = modalByTitle(wrapper, "交易详情");
    expect(modal.props("show")).toBe(true);
    expect(modal.findComponent(SplitDetail).props("split")).toMatchObject({ quantity: 2 });
    expect(visibleModalText()).toContain("000001 平安银行");
    expect(visibleModalText()).toContain("券商户");
  });

  it("带订单号的 buy 行选详情：订单区呈现（范围外修复 #1862 缺口——页签详情弹窗补 order 面）", async () => {
    ledgerDb = [
      { ...buyRow, source_order_no: "JD-9001" },
      sellRow,
      convertRow,
      splitRow,
      dividendRow,
    ];
    withCommandOverride("get_transaction_order_summary", () => ({
      source_order_no: "JD-9001",
      row_count: 1,
      total_amount_cents: 10000,
      currency_code: "CNY",
      accounts: [{ account_id: "acc-2", amount_cents: 10000 }],
      items: [],
    }));
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "detail");
    const modal = modalByTitle(wrapper, "交易详情");
    expect(modal.props("show")).toBe(true);
    expect(modal.findComponent(OrderDetail).exists()).toBe(true);
    expect(visibleModalText()).toContain("所属订单");
    expect(visibleModalText()).toContain("JD-9001");
  });

  it("dividend 行选详情：无扩展读取同步开窗，归属标的/金额/到账账户/备注完整", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, 4);
    await selectRowMenu(wrapper, "detail");
    expect(mockInvoke.mock.calls.some(([cmd]) => cmd.startsWith("get_transaction_"))).toBe(false);
    const modal = modalByTitle(wrapper, "交易详情");
    expect(modal.props("show")).toBe(true);
    const detail = modal.findComponent(DividendDetail);
    expect(detail.props("transaction")).toMatchObject({
      id: "t-dividend",
      source: { kind: "instrument", display_name: "AAPL 苹果" },
      note: "年中分红",
    });
    expect(visibleModalText()).toContain("AAPL 苹果");
    expect(visibleModalText()).toContain("银行");
    expect(visibleModalText()).toContain("年中分红");
  });

  it("convert 取数失败不开窗：错误提示、详情弹窗不出现", async () => {
    withCommandOverride("get_transaction_convert", () => Promise.reject(new Error("boom")));
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, 2);
    await selectRowMenu(wrapper, "detail");
    expect(modalByTitle(wrapper, "交易详情").props("show")).toBe(false);
    expect(messageCalls().some((m) => m.method === "error" && m.text.includes("boom"))).toBe(true);
  });
});

describe("明细页签软删（issue #1781）", () => {
  it("buy 行删除：二次确认 → delete_transaction → 行消失、总数递减", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "delete");
    expect(dialogText()).toContain("确认删除该条交易");
    const callsBefore = ledgerCalls().length;
    withCommandOverride("delete_transaction", (args) => {
      ledgerDb = ledgerDb.filter((r) => r.id !== args?.id);
      return undefined;
    });
    await clickDialogButton("删除");
    await flushPromises();
    expect(deleteCalls()).toHaveLength(1);
    expect(lastInvokeArgs("delete_transaction")).toEqual({ id: "t-buy" });
    expect(ledgerDb).toHaveLength(4);
    expect(wrapper.find('[data-testid="ledger-total"]').text()).toBe("共 4 条");
    const kindCells = ledgerPane(wrapper)
      .findAll(`td[data-col-key="kind"]`)
      .map((c) => c.text());
    expect(kindCells).toEqual(["卖出", "转换", "份额调整", "分红"]);
    expect(messageCalls().some((m) => m.method === "success" && m.text.includes("已删除"))).toBe(
      true,
    );
    expect(ledgerCalls().length).toBeGreaterThan(callsBefore);
  });

  it("取消删除：不发删除命令、行集不动", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "delete");
    await clickDialogButton("取消");
    await flushPromises();
    expect(deleteCalls()).toHaveLength(0);
    expect(wrapper.find('[data-testid="ledger-total"]').text()).toBe("共 5 条");
  });

  it("码化守卫失败：错误提示原样透出（部分卖出守卫文案），行集不动", async () => {
    withCommandOverride("delete_transaction", () =>
      Promise.reject(new Error("存在部分卖出，不能删除该买入")),
    );
    const sink = makeFakeSink();
    registerToastSink(sink);
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "delete");
    await clickDialogButton("删除");
    await flushPromises();
    expect(sink.error.mock.calls.some(([text]) => text.includes("部分卖出"))).toBe(true);
    expect(wrapper.find('[data-testid="ledger-total"]').text()).toBe("共 5 条");
  });
});

describe("明细页签弹窗关闭（issue #1781）", () => {
  async function openMenuThenClose(modalTitle: string, menuKey: string, rowIndex: number) {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, rowIndex);
    await selectRowMenu(wrapper, menuKey);
    const modal = modalByTitle(wrapper, modalTitle);
    expect(modal.props("show")).toBe(true);
    modal.vm.$emit("update:show", false);
    await flushPromises();
    expect(modal.props("show")).toBe(false);
  }

  it("编辑弹窗显式关闭：意图清回空终态，弹窗不显示", async () => {
    await openMenuThenClose("编辑交易", "edit", 0);
  });

  it("详情弹窗显式关闭：意图清回空终态", async () => {
    await openMenuThenClose("交易详情", "detail", 4);
  });
});

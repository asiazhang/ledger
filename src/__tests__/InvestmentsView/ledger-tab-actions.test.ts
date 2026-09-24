import { describe, it, expect, vi, beforeEach } from "vitest";
import { flushPromises, type VueWrapper } from "@vue/test-utils";
import { NDataTable, NDropdown, NInput, NModal } from "naive-ui";
import { lastInvokeArgs, mockInvoke, wireInvokeSeam } from "@ledger/test-support/invoke-mock";
import InvestmentsView from "@/views/InvestmentsView.vue";
import InvestmentLedgerTab from "@/investment/InvestmentLedgerTab.vue";
import ConvertDetail from "@/investment/ConvertDetail.vue";
import SplitDetail from "@/investment/SplitDetail.vue";
import DividendDetail from "@/investment/DividendDetail.vue";
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

/** 买入：10 @ 10 元 + 手续费 1 元，带出资账户与备注（编辑回填防误抹断言的非空形态）。 */
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

/** 卖出：4 @ 12 元 + 手续费 0.5 元。 */
const sellRow = makeInvestmentLedgerRow({
  id: "t-sell",
  kind: "sell",
  date: "2026-03-04",
  amount_cents: 4800,
  trade: { quantity: 4, price_cents: 120000, fee_cents: 50 },
});

/** 转换 A → B：转出 2 份确认 200 元、转入 1 份确认 190 元。 */
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

/** 现金分红：300 元到账「银行」（账户端即到账账户），带备注（详情备注行断言）。 */
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

/** 投资域命令契约快照（与 ledger-tab.test.ts 同款；明细行集见 ledgerDb）。 */
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

/** 弹窗族取数应答（编辑买卖明细 / convert 两腿 / split 份额调整，命令已存在 #1778）。 */
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
      get_transaction_trade: () => Promise.resolve(tradeDetail),
      get_transaction_convert: () => Promise.resolve(convertDetail),
      get_transaction_split: () => Promise.resolve(splitDetail),
    },
    refreshReferenceStores: true,
  }).ready;
});

// —— 页签内查找助手（同 ledger-tab.test.ts：先定位明细页签组件再向其子树查找）——

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
  // 明细页签仅行菜单一个 NDropdown；按行菜单专属项识别（delete / detail）
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

/** 菜单选择（走 NDropdown 的 onSelect 装配缝，TransactionsView 同款窄化）。 */
async function selectRowMenu(wrapper: VueWrapper, key: string) {
  fireProp(rowMenu(wrapper), "onSelect", key);
  await flushPromises();
}

/** 弹窗：按 title 定位（编辑/详情两弹窗同构，title 即用户可观察锚）。 */
function modalByTitle(wrapper: VueWrapper, title: string) {
  return wrapper.findAllComponents(NModal).find((m) => m.props("title") === title)!;
}

function ledgerCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === "list_investment_transactions");
}

function deleteCalls() {
  return mockInvoke.mock.calls.filter(([cmd]) => cmd === "delete_transaction");
}

/** 委托桩（一次性 Once 版被 mount 期的首个 invoke 消耗掉，此处用持续实现）：
 * 指定命令走覆写（可拒绝/可改库），其余委托回接缝分发器；每测 beforeEach
 * 重走唯一接缝布线，本覆写随用例结束自然失效。 */
function withCommandOverride(cmd: string, handler: (args?: Record<string, unknown>) => unknown) {
  const base = mockInvoke.getMockImplementation()!;
  mockInvoke.mockImplementation((c: string, args?: Record<string, unknown>) =>
    c === cmd ? Promise.resolve(handler(args)) : base(c, args),
  );
}

/**
 * 行操作同权——菜单项闭集与行激活闭集（ADR-0135 决策 4 / issue #1781）：
 * buy/sell = 编辑 + 软删、convert/split/dividend = 仅只读详情（界面只读 kind
 * 不出现编辑/软删入口，界面边界不变）、refund 不在行集。选项组装复用
 * transaction-row-menu 单点（行激活闭集单源 transactionKindActivation）。
 * 删除行菜单组装重接线即红。
 */
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
    // 操作列末位（列形态单点）且每行一枚「⋯」
    const cols = ledgerPane(wrapper).findComponent(NDataTable).props("columns") as Array<{
      key?: string;
    }>;
    expect(cols[cols.length - 1].key).toBe("actions");
    expect(ledgerPane(wrapper).findAll(".row-actions-btn")).toHaveLength(5);
    // convert 行：右键集合 = 「⋯」集合（仅「详情」，无编辑/软删入口）
    await openMenuOnRow(wrapper, 2);
    const rightClickKeys = rowMenuKeys(wrapper);
    await ledgerPane(wrapper).findAll(".row-actions-btn")[2].trigger("click");
    await flushPromises();
    expect(rowMenuKeys(wrapper)).toEqual(rightClickKeys);
  });
});

/**
 * 编辑 buy/sell（弹窗族复用，issue #1781）：先取买卖明细再开窗、失败不开窗的
 * 时序内化在 TransactionModalState（模块专测已覆盖时序与竞态），此处断言
 * 用户可观察结果——弹窗打开、表单回填、提交走 update_transaction、成功关窗保持
 * 当前页重拉。删除编辑重接线即红。
 */
describe("明细页签编辑 buy/sell（issue #1781）", () => {
  /** 编辑弹窗：按 title 定位。 */
  function editModal(wrapper: VueWrapper) {
    return modalByTitle(wrapper, "编辑交易");
  }

  it("buy 行选编辑：先取买卖明细，弹窗打开且投资表单回填标的/数量/备注/出资账户", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "edit");
    // 先取扩展明细再开窗：取数入参即目标行 id
    expect(lastInvokeArgs("get_transaction_trade")).toEqual({ id: "t-buy" });
    expect(editModal(wrapper).props("show")).toBe(true);
    // kind 锁死分派到投资表单（无类型切换）
    expect(wrapper.findAllComponents(InvestmentForm)).toHaveLength(1);
    const form = wrapper.findComponent(InvestmentForm);
    expect(form.props("editing")).toMatchObject({ id: "t-buy" });
    expect(form.props("trade")).toMatchObject({ symbol: "600000", quantity: 10 });
    // 回填备注（NInput value；弹窗行适配携带，防止全字段替换静默抹掉原备注）
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
    // 一次性委托桩（接缝钦定形态）：领域命令自接，其余委托回接缝分发器
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
    // 弹窗关闭、明细列表以当前状态重拉（保持当前页与筛选）
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
    // 失败不开窗且错误提示上屏（稳定替身实例断言面）
    expect(messageCalls().some((m) => m.method === "error" && m.text.includes("无买卖明细"))).toBe(
      true,
    );
  });
});

/**
 * 只读详情 convert/split/dividend（弹窗族复用，issue #1781）：convert/split
 * 先取扩展明细再开窗、dividend 同步开窗（时序内化在 TransactionModalState）。
 * 三种只读详情完整呈现（两腿 / 带符号 Δ / 标的+金额+到账账户+备注），断言对准
 * 弹窗可见文本。删除详情重接线即红。
 */
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
    // NModal teleport 到 body，内容文本走 document 可见卡片（test-support 单点）
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

  it("dividend 行选详情：无扩展读取同步开窗，归属标的/金额/到账账户/备注完整", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, 4);
    await selectRowMenu(wrapper, "detail");
    // dividend 无扩展读取命令（同步落意图作渲染面判别，编排内化）
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

/**
 * 软删（issue #151 二次确认同款 / issue #1781 照常）：确认后走既有
 * delete_transaction，行消失且总数递减；既有级联与码化守卫在后端随命令自然生效，
 * 失败提示原样透出（不新增错误码）。删除软删重接线即红。
 */
describe("明细页签软删（issue #1781）", () => {
  it("buy 行删除：二次确认 → delete_transaction → 行消失、总数递减", async () => {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "delete");
    // 二次确认弹窗（useAppDialog）：取消不删
    expect(dialogText()).toContain("确认删除该条交易");
    const callsBefore = ledgerCalls().length;
    withCommandOverride("delete_transaction", (args) => {
      ledgerDb = ledgerDb.filter((r) => r.id !== args?.id);
      return undefined;
    });
    await clickDialogButton("删除");
    await flushPromises();
    // 删除命令以行 id 发出（级联与守卫归后端，照常生效）
    expect(deleteCalls()).toHaveLength(1);
    expect(lastInvokeArgs("delete_transaction")).toEqual({ id: "t-buy" });
    // 行消失 + 总数递减（用户可观察双断言）
    expect(ledgerDb).toHaveLength(4);
    expect(wrapper.find('[data-testid="ledger-total"]').text()).toBe("共 4 条");
    const kindCells = ledgerPane(wrapper)
      .findAll(`td[data-col-key="kind"]`)
      .map((c) => c.text());
    expect(kindCells).toEqual(["卖出", "转换", "份额调整", "分红"]);
    // 删除成功提示 + 重拉发生
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
    // 软删错误反馈走 Loadable 统一通道（#1008 / #1039 守门），断言只看 sink 面
    const sink = makeFakeSink();
    registerToastSink(sink);
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, 0);
    await selectRowMenu(wrapper, "delete");
    await clickDialogButton("删除");
    await flushPromises();
    // 守卫错误经统一 toast 通道原样透出（码化文案不加工，Loadable sink 断言面）
    expect(sink.error.mock.calls.some(([text]) => text.includes("部分卖出"))).toBe(true);
    // 行未消失（总数不变）
    expect(wrapper.find('[data-testid="ledger-total"]').text()).toBe("共 5 条");
  });
});

/**
 * 弹窗显式关闭（✕ / ESC 同一路径）：意图清回空终态——关闭语义由编排内化，
 * 本页签只接线；关闭编排语义由 TransactionModalState 模块专测与 TransactionsView
 * 既有测试（保持绿）共同钉住。
 */
describe("明细页签弹窗关闭（issue #1781）", () => {
  async function openMenuThenClose(modalTitle: string, menuKey: string, rowIndex: number) {
    const wrapper = mountView();
    await flushPromises();
    await openLedgerTab(wrapper);
    await openMenuOnRow(wrapper, rowIndex);
    await selectRowMenu(wrapper, menuKey);
    const modal = modalByTitle(wrapper, modalTitle);
    expect(modal.props("show")).toBe(true);
    // 用户点遮罩/关闭 → update:show=false（add-record 同款装配缝直发）
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

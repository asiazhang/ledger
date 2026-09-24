import type {
  Account,
  Category,
  Currency,
  DashboardOverview,
  FinancialFreedomOverview,
  Holding,
  Instrument,
  InvestmentOverview,
  InvestmentTransactionRow,
  MoneyWeightedReturnSummary,
  ItemDailyTotal,
  PhysicalAsset,
  PhysicalAssetList,
  ParkedOpInfo,
  Policy,
  PolicyStats,
  RealizedPnlSummary,
  SavingsGoal,
  SavingsGoalProgress,
  SyncChannelConfig,
  SyncRoundReport,
  SyncStatus,
  Transaction,
} from "@ledger/types";

// 计划实体工厂三形态 + 期次工厂自 #1322 起上收共享测试支持包
// （@ledger/test-support/plan-factories）：抽 @ledger/scheduled-plan-list 时包内
// 测试跟随被测包，结构守门规则 4 禁测试文件本地定义名单工厂，共享工厂层出口
// 随测试面上收；此处再导出保持壳侧既有 import 面（'./factories'）不变。
export {
  makeSubscriptionPlan,
  makeInstallmentPlan,
  makeTransferPlan,
  makeOccurrence,
} from "@ledger/test-support/plan-factories";

// toast sink 假件自 #1354 起上收共享测试支持包（@ledger/test-support/toast-sink）：
// @ledger/loadable 与 @ledger/scheduled-plan-list 包内测试不可引用壳侧 factories
// （结构守门规则③），同实现局部替身随测试面上收，唯一定义点在包内；
// 此处再导出保持壳侧既有 import 面（'./factories'）不变。
import { registerToastSink } from "@ledger/loadable";
import {
  makeFakeSink,
  resetToastSink as resetToastSinkSupport,
} from "@ledger/test-support/toast-sink";

export { makeFakeSink };

export function resetToastSink(): void {
  resetToastSinkSupport(registerToastSink);
}

/**
 * 组件/composable 测试的共享数据工厂（issue #110 审查：消除测试文件间重复）。
 * invoke 布线一律走唯一接缝 wireInvokeSeam（@ledger/test-support/invoke-mock.ts，ADR-0085），
 * 本文件只承载数据夹具（toast sink 假件见 @ledger/test-support/toast-sink），不含任何布线能力。
 */

export const mockCurrencies: Currency[] = [
  { code: "CNY", name: "人民币", symbol: "¥", decimal_places: 2 },
];

export function makeAccount(partial: Partial<Account> & { id: string }): Account {
  return {
    name: "证券账户A",
    type: "investment",
    currency_code: "CNY",
    initial_balance_cents: 0,
    created_at: "2026-01-01T00:00:00Z",
    is_hidden: false,
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    ...partial,
  };
}

export const mockAccounts: Account[] = [makeAccount({ id: "acc-1" })];

export function makeCategory(partial: Partial<Category> & { id: string }): Category {
  return {
    name: partial.id,
    kind: "expense",
    parent_id: null,
    icon: null,
    sort_order: 0,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    ...partial,
  };
}

export function makeInstrument(partial: Partial<Instrument> & { id: string }): Instrument {
  return {
    symbol: "600000",
    type: "stock",
    name: "浦发银行",
    currency_code: "CNY",
    market: "sh",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    source: "eastmoney",
    price_cents: null,
    invested: true,
    price_channel: "quote",
    ...partial,
  };
}

export function makeHolding(
  partial: Partial<Holding> & { id: string; instrument_id: string },
): Holding {
  return {
    account_id: "acc-1",
    quantity: 100,
    cost_basis_cents: 120000,
    cost_currency_code: "CNY",
    latest_price_cents: null,
    latest_price_currency_code: null,
    latest_nav_date: null,
    market_value_cents: null,
    unrealized_pnl_cents: null,
    // 折本位币两列（issue #1797）：缺价行默认 null；有行情行由用例按折算结果给值
    native_market_value_cents: null,
    native_unrealized_pnl_cents: null,
    updated_at: "2026-01-01T00:00:00Z",
    ...partial,
  };
}

/**
 * 投资明细行工厂（ADR-0135 / issue #1779）：五种投资 kind 的投资投影行底座——
 * 默认 buy 行形态（trade 载荷按需携带，convert/split 缺省 null），公共字段中性。
 */
export function makeInvestmentLedgerRow(
  partial: Partial<InvestmentTransactionRow> & {
    id: string;
    kind: InvestmentTransactionRow["kind"];
  },
): InvestmentTransactionRow {
  return {
    date: "2026-03-01",
    amount_cents: 10000,
    account_id: "acc-1",
    funding_account_id: null,
    instrument_id: "inst-1",
    symbol: "600000",
    instrument_name: "浦发银行",
    instrument_type: "stock",
    trade: null,
    convert: null,
    split: null,
    ...partial,
  };
}

/** h-1 有行情（价格/市值/未实现盈亏齐全，账户币 = 本位币故折算值同额），h-2 无行情（全 NULL） */
export const mockHoldings: Holding[] = [
  makeHolding({
    id: "h-1",
    instrument_id: "inst-1",
    quantity: 100,
    cost_basis_cents: 120000,
    latest_price_cents: 150000,
    latest_price_currency_code: "CNY",
    market_value_cents: 150000,
    unrealized_pnl_cents: 30000,
    native_market_value_cents: 150000,
    native_unrealized_pnl_cents: 30000,
  }),
  makeHolding({
    id: "h-2",
    instrument_id: "inst-2",
    quantity: 10,
    cost_basis_cents: 8000,
  }),
];

export const mockInstruments: Instrument[] = [
  makeInstrument({ id: "inst-1" }),
  makeInstrument({ id: "inst-2", symbol: "000001", name: "平安银行", market: "sz" }),
];

/** dashboard_overview 返回值工厂（issue #143）：默认人民币本位币、净申 1234.56 元 */
/** 保单实体工厂（issue #360）：保单 store 与视图测试共用（消除本地复制）。 */
export function makePolicy(partial: Partial<Policy> & { id: string }): Policy {
  return {
    insurer_id: "ins-1",
    policy_number: "P2026-001",
    product_name: "重疾险",
    start_date: "2024-01-01",
    end_date: "2036-01-01",
    coverage_amount_cents: 30_000_000,
    coverage_currency_code: "CNY",
    note: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    ...partial,
  };
}

export function makePolicyStats(partial: Partial<PolicyStats> = {}): PolicyStats {
  return {
    policy_id: "policy-1",
    native_currency: "CNY",
    total_paid_native_cents: 600_000,
    total_inflow_native_cents: 50_000,
    next_charge_date: null,
    is_expired: false,
    ...partial,
  };
}

export function makeOverview(partial: Partial<DashboardOverview> = {}): DashboardOverview {
  return {
    native_currency: "CNY",
    net_worth_cents: 123456,
    accounts_balance_cents: 100000,
    holdings_market_value_cents: 23456,
    physical_assets_value_cents: 0,
    ...partial,
  };
}

/** 交易行工厂：默认一笔 100 元人民币支出，覆写见 partial（id 必填）。 */
export function makeTransaction(partial: Partial<Transaction> & { id: string }): Transaction {
  return {
    kind: "expense",
    amount_cents: 10000,
    currency_code: "CNY",
    amount_native_cents: 10000,
    fx_rate_used: null,
    fx_rate_source: null,
    account_id: "acc-1",
    to_account_id: null,
    funding_account_id: null,
    category_id: null,
    merchant_id: null,
    policy_id: null,
    refund_of_transaction_id: null,
    note: null,
    date: "2026-01-01",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    // 来源列默认无来源（列表/搜索读路径才填充）；来源场景显式传 source
    source: null,
    // 转换扩展默认无（仅 convert 行由列表/搜索读路径填充）；转换场景显式传 convert
    convert: null,
    ...partial,
  };
}

/**
 * item_daily_total 返回值工厂（issue #122）：默认人民币本位币、每天成本 123.45 元、3 件在用 */
export function makeItemDailyTotal(partial: Partial<ItemDailyTotal> = {}): ItemDailyTotal {
  return { native_currency: "CNY", per_day_cents: 12345, item_count: 3, ...partial };
}

/** financial_freedom 返回值工厂（issue #344）：默认人民币本位币、自由度 7.5%
 * （可投资资产 5000 元 × 3% ÷ 年度预算 2 万）、覆盖 0.3 年 */
export function makeFinancialFreedom(
  partial: Partial<FinancialFreedomOverview> = {},
): FinancialFreedomOverview {
  return {
    ratio: 7.5,
    numerator_cents: 500000,
    denominator_cents: 2000000,
    coverage_years: 0.3,
    native_currency: "CNY",
    ...partial,
  };
}

/** realized_pnl_summary 返回值工厂（issue #325；ADR-0107 起按币种分组、无明细；
 * ADR-0129 起按年/按账户行各带分红腿与合计）：默认全表汇总 300 元（CNY）、无分红 */
export function makePnlSummary(partial: Partial<RealizedPnlSummary> = {}): RealizedPnlSummary {
  return {
    total: [{ currency_code: "CNY", realized_pnl_cents: 30000 }],
    by_year: [
      {
        year: "2026",
        currency_code: "CNY",
        realized_pnl_cents: 30000,
        dividend_cents: 0,
        realized_gain_cents: 30000,
      },
    ],
    by_account: [
      {
        account_id: "acc-1",
        account_name: "证券账户A",
        currency_code: "CNY",
        realized_pnl_cents: 30000,
        dividend_cents: 0,
        realized_gain_cents: 30000,
      },
    ],
    by_instrument: [
      {
        instrument_id: "inst-1",
        symbol: "600000",
        name: "浦发银行",
        currency_code: "CNY",
        realized_pnl_cents: 30000,
      },
    ],
    ...partial,
  };
}

/** money_weighted_return_summary 返回值工厂（issue #1195 / ADR-0115）：默认
 * 单标的 +10%、账户 +10%、全账 CNY +10%；null 率（无解）与缺行（缺价跳过）
 * 由用例经 partial / 覆写表达三态。 */
export function makeMwrSummary(
  partial: Partial<MoneyWeightedReturnSummary> = {},
): MoneyWeightedReturnSummary {
  return {
    by_instrument: [
      {
        account_id: "acc-1",
        instrument_id: "inst-1",
        currency_code: "CNY",
        basis: "annualized",
        rate: 0.1,
      },
    ],
    by_account: [
      {
        account_id: "acc-1",
        account_name: "证券账户A",
        currency_code: "CNY",
        basis: "annualized",
        rate: 0.1,
      },
    ],
    total: [{ currency_code: "CNY", basis: "annualized", rate: 0.1 }],
    ...partial,
  };
}

/** 实物资产实体夹具（issue #466）：全字段读模型 + 当前估值三件套。 */
export function makePhysicalAsset(partial: Partial<PhysicalAsset> & { id: string }): PhysicalAsset {
  return {
    name: "客厅油画",
    purchase_date: null,
    purchase_price_cents: null,
    purchase_currency_code: null,
    status: "holding",
    disposal_date: null,
    disposal_price_cents: null,
    disposal_currency_code: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    current_valuation_cents: 5_000_000,
    current_valuation_currency_code: "CNY",
    current_valuation_date: "2026-01-01",
    current_valuation_native_cents: 5_000_000,
    native_currency: "CNY",
    ...partial,
  };
}

/** 储蓄目标实体夹具（spec #1750 / issue #1751）：全字段读模型（金额整数分）。 */
export function makeSavingsGoal(partial: Partial<SavingsGoal> & { id: string }): SavingsGoal {
  return {
    name: "买车基金",
    target_amount_cents: 5_000_000,
    deadline: "2027-06-30",
    status: "active",
    planned_monthly_cents: null,
    account_id: `acc-${partial.id}`,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    ...partial,
  };
}

/** 蓄水进度行夹具（spec #1750 / issue #1751 / #1753）：目标行 + 已存 / 还差 / 达成态 /
 *  币种 + 双向推算。还差缺省由目标额 − 已存派生（夹具自洽）；推算字段缺省
 *  节奏为零（全 null，不虚构时点），场景给显式值优先。 */
export function makeSavingsGoalProgress(
  over: Partial<SavingsGoalProgress> & {
    goal?: Partial<SavingsGoal> & { id: string };
  } = {},
): SavingsGoalProgress {
  const { goal, ...rest } = over;
  const resolvedGoal = makeSavingsGoal(goal ?? { id: "goal-1" });
  const savedCents = rest.saved_cents ?? 0;
  return {
    goal: resolvedGoal,
    saved_cents: savedCents,
    remaining_cents: rest.remaining_cents ?? resolvedGoal.target_amount_cents - savedCents,
    achieved: rest.achieved ?? false,
    currency_code: rest.currency_code ?? "CNY",
    pace_monthly_cents: rest.pace_monthly_cents ?? null,
    pace_source: rest.pace_source ?? null,
    eta_months: rest.eta_months ?? null,
    eta_month: rest.eta_month ?? null,
    required_monthly_cents: rest.required_monthly_cents ?? null,
    pace_delta_cents: rest.pace_delta_cents ?? null,
  };
}

/** 目标专属账户夹具（issue #1755 / ADR-0133 决策 2）：目标账户是 `other` 类型账户，
 *  特殊性不靠类型值表达——是否目标户由绑定派生，本工厂只造账户本体。 */
export function makeGoalBoundAccount(partial: Partial<Account> & { id: string }): Account {
  return makeAccount({ type: "other", ...partial });
}

/** 目标 + 专属账户成对夹具（issue #1755）：一次造出绑定两端——账户与进度行同源
 *  （goal.account_id 指向本工厂账户 id），新测试一律走成对厂、不手拼 account_id
 *  （makeSavingsGoal 的 account_id 缺省是 `acc-${goal.id}`，与账户 id 无关，易错位）。 */
export function makeGoalPair(
  options: {
    /** 专属账户 id（绑定目标 account_id 的锚点）。 */
    id: string;
    /** 目标本体覆写（id 缺省 goal-1；account_id 一律指向本工厂账户，不可覆写错位）。 */
    goal?: Partial<SavingsGoal> & { id: string };
    /** 账户本体覆写（name / currency_code / 余额等）。 */
    account?: Partial<Account>;
  } = { id: "acc-goal" },
): { account: Account; progress: SavingsGoalProgress } {
  const account = makeGoalBoundAccount({ id: options.id, ...options.account });
  const progress = makeSavingsGoalProgress({
    goal: makeSavingsGoal({
      ...options.goal,
      id: options.goal?.id ?? "goal-1",
      account_id: account.id,
    }),
  });
  return { account, progress };
}

/** 实物资产列表返回夹具（资产行 + 在持合计同源快照）。 */
export function makePhysicalAssetList(partial: Partial<PhysicalAssetList> = {}): PhysicalAssetList {
  return {
    assets: [],
    holding_total_native_cents: 0,
    native_currency: "CNY",
    ...partial,
  };
}

/** 多端同步状态夹具（issue #862）：明文库 + 已配置通道 + 从未挂起。 */
export function makeSyncStatus(partial: Partial<SyncStatus> = {}): SyncStatus {
  return {
    device_id: "device-abcdef",
    channel_configured: true,
    last_sync_at: "2026-01-15T08:30:00Z",
    parked_count: 0,
    library_encrypted: false,
    ...partial,
  };
}

/** 同步通道配置回显夹具（issue #1218）：已配置形态。 */
export function makeSyncChannelConfig(partial: Partial<SyncChannelConfig> = {}): SyncChannelConfig {
  return {
    space_id: "family",
    endpoint: "https://s3.example.com",
    region: "us-east-1",
    bucket: "ledger-bucket",
    prefix: "sync",
    access_key: "AKIAEXAMPLE",
    secret_key: "secret-value",
    path_style: true,
    configured: true,
    ...partial,
  };
}

/** 挂起操作明细夹具（issue #863）：带码化原因（schema-ahead）。 */
export function makeParkedOp(partial: Partial<ParkedOpInfo> = {}): ParkedOpInfo {
  return {
    op_id: "op-1",
    device_id: "device-abcdef",
    entity: "transaction",
    entity_id: "txn-1",
    code: "sync-engine.schema-ahead",
    params: [],
    message: "该操作来自更新版本的应用，升级本端后将自动重试",
    parked_at: "2026-01-15T09:00:00Z",
    ...partial,
  };
}

/** 同步轮次报告夹具（issue #862）：上传 1 段 2 条、全部应用、无挂起。 */
export function makeSyncRoundReport(partial: Partial<SyncRoundReport> = {}): SyncRoundReport {
  return {
    uploaded_segments: 1,
    uploaded_ops: 2,
    downloaded_segments: 0,
    applied: 0,
    deduped: 0,
    superseded: 0,
    skipped: 0,
    parked: 0,
    plaintext_mode: true,
    ...partial,
  };
}

/** `investment_overview` 返回值工厂（spec #1532 / issue #1536）：默认无投资资产
 *（两腿与合计为 0、无缺料说明、已建投资账户）；缺价计数、无投资账户与两腿数值
 * 由用例经 partial 表达。 */
export function makeInvestmentOverview(
  partial: Partial<InvestmentOverview> = {},
): InvestmentOverview {
  return {
    native_currency: "CNY",
    investable_assets_cents: 0,
    investment_cash_cents: 0,
    holdings_market_value_cents: 0,
    total_market_value_cents: 0,
    unrealized_pnl_cents: 0,
    cumulative_pnl_cents: 0,
    missing_price_holding_count: 0,
    has_investment_account: true,
    ...partial,
  };
}

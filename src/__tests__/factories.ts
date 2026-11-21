import type {
  Account,
  Category,
  DashboardOverview,
  FinancialFreedomOverview,
  Holding,
  Instrument,
  InvestmentOverview,
  InvestmentTransactionRow,
  Merchant,
  MoneyWeightedReturnSummary,
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
  TransactionFunding,
  TransactionPurchase,
} from "@ledger/types";

export {
  makeSubscriptionPlan,
  makeInstallmentPlan,
  makeTransferPlan,
  makeOccurrence,
} from "@ledger/test-support/plan-factories";

import { registerToastSink } from "@ledger/loadable";
import {
  makeFakeSink,
  resetToastSink as resetToastSinkSupport,
} from "@ledger/test-support/toast-sink";

export { makeFakeSink };

export function resetToastSink(): void {
  resetToastSinkSupport(registerToastSink);
}

export function makeAccount(partial: Partial<Account> & { id: string }): Account {
  return {
    name: "证券账户A",
    type: "investment",
    currency_code: "CNY",
    initial_balance_cents: 0,
    created_at: "2026-01-01T00:00:00Z",
    is_hidden: false,
    is_archived: false,
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

export function makeMerchant(partial: Partial<Merchant> & { id: string }): Merchant {
  return {
    name: "京东",
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
    native_market_value_cents: null,
    native_unrealized_pnl_cents: null,
    updated_at: "2026-01-01T00:00:00Z",
    ...partial,
  };
}

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
    source_order_no: null,
    instrument_id: "inst-1",
    symbol: "600000",
    instrument_name: "浦发银行",
    instrument_type: "stock",
    trade: null,
    convert: null,
    split: null,
    note: null,
    currency_code: "CNY",
    amount_native_cents: 10000,
    ...partial,
  };
}

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
    fundings: [],
    purchases: [],
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
    source: null,
    source_order_no: null,
    convert: null,
    ...partial,
  };
}

export function makeFunding(partial: Partial<TransactionFunding> = {}): TransactionFunding {
  return {
    account_id: "acc-1",
    amount_cents: 10000,
    label: null,
    derived: false,
    ...partial,
  };
}

export function makePurchase(partial: Partial<TransactionPurchase> = {}): TransactionPurchase {
  return {
    name: "商品",
    quantity: 1,
    category_id: null,
    unit_price_cents: null,
    ...partial,
  };
}

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

export function makePnlSummary(partial: Partial<RealizedPnlSummary> = {}): RealizedPnlSummary {
  return {
    by_year: [
      {
        year: "2026",
        native_currency: "CNY",
        realized_pnl_cents: 30000,
        dividend_cents: 0,
        realized_gain_cents: 30000,
      },
    ],
    by_account: [
      {
        account_id: "acc-1",
        account_name: "证券账户A",
        native_currency: "CNY",
        realized_pnl_cents: 30000,
        dividend_cents: 0,
        realized_gain_cents: 30000,
      },
    ],
    ...partial,
  };
}

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

function makeGoalBoundAccount(partial: Partial<Account> & { id: string }): Account {
  return makeAccount({ type: "other", ...partial });
}

export function makeGoalPair(
  options: {
    id: string;
    goal?: Partial<SavingsGoal> & { id: string };
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

export function makePhysicalAssetList(partial: Partial<PhysicalAssetList> = {}): PhysicalAssetList {
  return {
    assets: [],
    holding_total_native_cents: 0,
    native_currency: "CNY",
    ...partial,
  };
}

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

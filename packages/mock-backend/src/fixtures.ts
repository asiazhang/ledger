import type {
  Account,
  Budget,
  Category,
  Currency,
  Instrument,
  Insurer,
  Merchant,
  SavingsGoal,
  Transaction,
} from "@ledger/types";
import type { MockAssetValuation, MockPhysicalAsset, MockSecurityTxn } from "./state";

/**
 * 确定性小夹具（issue #1962 / ADR-0143 决策 6）：参考数据五字典 + 覆盖主要 kind
 * 形态的少量交易，提交进仓库随代码维护——开发迭代要的是可复现起点，不是拟真；
 * 要看更多数据就在本文件加行。字段形态是各命令契约的投影，命令契约演化后夹具
 * 字段过时由消费该命令时的报错/类型错暴露（ADR-0143「代价」节，接受漂移）。
 *
 * 确定性纪律：全部 id / 时间戳 / 金额为字面量，不含 `Date.now()` 等运行期求值——
 * 刷新页面（重新播种）后状态逐字节回到本文件所写。
 */

const T0 = "2026-01-01T00:00:00Z";

function refRow(device = "mock"): {
  updated_at: string;
  version: number;
  device_id: string;
  is_deleted: boolean;
} {
  return { updated_at: T0, version: 1, device_id: device, is_deleted: false };
}

// —— 参考数据五字典（四张参考表 + 保司字典，ADR-0082） ——

export const fixtureCurrencies: Currency[] = [
  { code: "CNY", name: "人民币", symbol: "¥", decimal_places: 2 },
  { code: "USD", name: "美元", symbol: "$", decimal_places: 2 },
];

export const fixtureAccounts: Account[] = [
  {
    id: "mock-acc-cash",
    name: "现金钱包",
    type: "cash",
    currency_code: "CNY",
    initial_balance_cents: 50_000,
    created_at: T0,
    is_hidden: false,
    is_archived: false,
    ...refRow(),
  },
  {
    id: "mock-acc-bank",
    name: "工资卡",
    type: "bank",
    currency_code: "CNY",
    initial_balance_cents: 1_000_000,
    created_at: T0,
    is_hidden: false,
    is_archived: false,
    ...refRow(),
  },
  {
    id: "mock-acc-ewallet",
    name: "支付钱包",
    type: "ewallet",
    currency_code: "CNY",
    initial_balance_cents: 20_000,
    created_at: T0,
    is_hidden: false,
    is_archived: false,
    ...refRow(),
  },
  {
    id: "mock-acc-credit",
    name: "信用卡",
    type: "credit",
    currency_code: "CNY",
    initial_balance_cents: 0,
    credit_limit_cents: 300_000,
    statement_day: 5,
    due_day: 25,
    created_at: T0,
    is_hidden: false,
    is_archived: false,
    ...refRow(),
  },
  {
    id: "mock-acc-invest",
    name: "券商账户",
    type: "investment",
    currency_code: "CNY",
    initial_balance_cents: 0,
    created_at: T0,
    is_hidden: false,
    is_archived: false,
    ...refRow(),
  },
  // 储蓄目标「应急基金」的专属账户（other 类型，issue #1975）：期初即蓄水 300_000
  {
    id: "mock-acc-goal-emergency",
    name: "应急基金",
    type: "other",
    currency_code: "CNY",
    initial_balance_cents: 300_000,
    created_at: "2025-12-01T09:00:00Z",
    is_hidden: false,
    is_archived: false,
    ...refRow(),
  },
];

export const fixtureCategories: Category[] = [
  {
    id: "mock-cat-food",
    name: "餐饮",
    kind: "expense",
    parent_id: null,
    icon: null,
    sort_order: 0,
    created_at: T0,
    ...refRow(),
  },
  {
    id: "mock-cat-food-breakfast",
    name: "早餐",
    kind: "expense",
    parent_id: "mock-cat-food",
    icon: null,
    sort_order: 0,
    created_at: T0,
    ...refRow(),
  },
  {
    id: "mock-cat-transport",
    name: "交通",
    kind: "expense",
    parent_id: null,
    icon: null,
    sort_order: 1,
    created_at: T0,
    ...refRow(),
  },
  {
    id: "mock-cat-salary",
    name: "工资",
    kind: "income",
    parent_id: null,
    icon: null,
    sort_order: 2,
    created_at: T0,
    ...refRow(),
  },
  {
    id: "mock-cat-del",
    name: "已删分类",
    kind: "expense",
    parent_id: null,
    icon: null,
    sort_order: 3,
    created_at: T0,
    ...refRow(),
    is_deleted: true,
  },
];

export const fixtureMerchants: Merchant[] = [
  { id: "mock-mch-meituan", name: "美团", ...refRow() },
  { id: "mock-mch-jd", name: "京东", ...refRow() },
  { id: "mock-mch-del", name: "已删商户", ...refRow(), is_deleted: true },
];

export const fixtureInsurers: Insurer[] = [
  { id: "mock-ins-picc", name: "中国人保", ...refRow() },
  { id: "mock-ins-pingan", name: "中国平安", ...refRow() },
];

// —— 实物资产（issue #1975）：在持 / 已处置各一行，估值历史见 fixtureAssetValuations ——
// 当前估值不落行：黄金首饰 = 最新一条 2026-01-10 的 132_000；已处置行 native 投影缺席。

export const fixturePhysicalAssets: MockPhysicalAsset[] = [
  {
    id: "mock-pa-gold",
    name: "黄金首饰",
    purchase_date: "2025-06-15",
    purchase_price_cents: 120_000,
    purchase_currency_code: "CNY",
    status: "holding",
    disposal_date: null,
    disposal_price_cents: null,
    disposal_currency_code: null,
    created_at: "2025-06-15T10:00:00Z",
    ...refRow(),
  },
  {
    id: "mock-pa-laptop",
    name: "笔记本电脑",
    purchase_date: "2024-03-01",
    purchase_price_cents: 150_000,
    purchase_currency_code: "CNY",
    status: "disposed",
    disposal_date: "2025-11-20",
    disposal_price_cents: 40_000,
    disposal_currency_code: "CNY",
    created_at: "2024-03-01T10:00:00Z",
    ...refRow(),
  },
];

export const fixtureAssetValuations: MockAssetValuation[] = [
  {
    id: "mock-pav-gold-1",
    asset_id: "mock-pa-gold",
    valuation_date: "2025-12-31",
    amount_cents: 130_000,
    currency_code: "CNY",
  },
  {
    id: "mock-pav-gold-2",
    asset_id: "mock-pa-gold",
    valuation_date: "2026-01-10",
    amount_cents: 132_000,
    currency_code: "CNY",
  },
  {
    id: "mock-pav-laptop-1",
    asset_id: "mock-pa-laptop",
    valuation_date: "2024-12-31",
    amount_cents: 90_000,
    currency_code: "CNY",
  },
  {
    id: "mock-pav-laptop-2",
    asset_id: "mock-pa-laptop",
    valuation_date: "2025-11-01",
    amount_cents: 42_000,
    currency_code: "CNY",
  },
];

// —— 储蓄目标（issue #1975）：进行中一行，专属账户 mock-acc-goal-emergency 蓄水
// 300_000（期初直记，无流水——进度读数 saved = 账户余额直和）；进度推算的独立
// 人算期望值见 savings-goal.test.ts 头注（ETA 7 个月 / 2026-08）。 ——

export const fixtureSavingsGoals: SavingsGoal[] = [
  {
    id: "mock-goal-emergency",
    name: "应急基金",
    target_amount_cents: 1_000_000,
    deadline: null,
    status: "active",
    planned_monthly_cents: 100_000,
    account_id: "mock-acc-goal-emergency",
    created_at: "2025-12-01T09:00:00Z",
    ...refRow(),
  },
];

// —— 账本级设置（issue #1965 首批：本位币基准缺省 CNY / 日志等级缺省 info，设置域读写的种子态） ——

export const fixtureSettings = {
  base_currency: "CNY",
  log_level: "info",
};

// —— 预算（issue #1967：月度/年度两类种子行，引用在用分类；聚合窗口锚见 budget.ts） ——

export const fixtureBudgets: Budget[] = [
  {
    id: "mock-bud-food-monthly",
    category_id: "mock-cat-food",
    period: "monthly",
    amount_cents: 200_000,
    start_date: "2026-01-01",
    created_at: T0,
    ...refRow(),
  },
  {
    id: "mock-bud-transport-yearly",
    category_id: "mock-cat-transport",
    period: "yearly",
    amount_cents: 1_200_000,
    start_date: "2026-01-01",
    created_at: T0,
    ...refRow(),
  },
];

// —— 投资标的字典（issue #1969）：股/基/ETF/债券/自建多市场种子行，覆盖手动/
// 同步两种来源与现价有/无两形态；`invested` 与 `price_channel` 真后端为派生列
//（持仓谓词 / 价格通道判定单点在 Rust 投资域），替身按行直投影（不追行为
// 等价，ADR-0143 决策 1；持仓域替身落地前 invested 无数据源，夹具即口径）。

export const fixtureInstruments: Instrument[] = [
  // symbol 排序（真后端 ORDER BY i.symbol）后种子序：000001 / 00700 / 019666 /
  // 159915 / 600519 / GOLD——筛选分页断言按此序人算
  {
    id: "mock-inst-fund",
    symbol: "000001",
    type: "fund",
    name: "华夏成长混合",
    currency_code: "CNY",
    market: "unknown", // 场外基金无交易所市场，恒 unknown（ADR-0038）
    created_at: T0,
    source: "eastmoney",
    price_cents: 12_345, // 1.2345 元净值（万分之一元刻度）
    invested: true,
    price_channel: "fund_nav",
    ...refRow(),
  },
  {
    id: "mock-inst-hk",
    symbol: "00700",
    type: "stock",
    name: "腾讯控股",
    currency_code: "HKD",
    market: "hk",
    created_at: T0,
    source: "eastmoney",
    price_cents: 385_000_000, // 38500.00 港元
    invested: true,
    price_channel: "quote",
    ...refRow(),
  },
  {
    id: "mock-inst-bond",
    symbol: "019666",
    type: "bond",
    name: "记账式国债",
    currency_code: "CNY",
    market: "sh",
    created_at: T0,
    source: "manual",
    price_cents: null, // 现价无（手动建档未录价形态）
    invested: false,
    price_channel: "none",
    ...refRow(),
  },
  {
    id: "mock-inst-etf",
    symbol: "159915",
    type: "etf",
    name: "创业板ETF",
    currency_code: "CNY",
    market: "sz",
    created_at: T0,
    source: "eastmoney",
    price_cents: 20_500, // 2.05 元
    invested: false,
    price_channel: "quote",
    ...refRow(),
  },
  {
    id: "mock-inst-sh",
    symbol: "600519",
    type: "stock",
    name: "贵州茅台",
    currency_code: "CNY",
    market: "sh",
    created_at: T0,
    source: "eastmoney",
    price_cents: 1_485_000_000, // 148500.00 元
    invested: true,
    price_channel: "quote",
    ...refRow(),
  },
  {
    id: "mock-inst-custom",
    symbol: "GOLD",
    type: "other",
    name: "家庭黄金存折",
    currency_code: "CNY",
    market: "unknown",
    created_at: T0,
    source: "manual",
    price_cents: 55_000, // 5.50 元/克折算占位（手动录价通道形态）
    invested: false,
    price_channel: "manual",
    ...refRow(),
  },
];

// —— 投资交易（issue #1970）：五种投资 kind 种子行（buy/sell/convert/split/
// dividend）+ 证券扩展行（引用 fixtureInstruments 标的 id 与既有账户）。金额
// 沿写路径先例人算（buy 非基金 = 数量 × 单价 + 费用；sell 基金 = 确认净额、
// 毛收入反算单价；convert 行金额锚点 = 结转成本字面量，不追 FIFO；split 无
// 现金腿恒 0）；余额口径沿用既有直和（ledger.ts：buy/sell/dividend 记
// funding_account_id ?? account_id，convert/split 无现金腿）。

export const fixtureSecurityTxns: MockSecurityTxn[] = [
  // buy：ETF 单价权威（非基金）——amount = 1000 × 2.05 元 + 2 元费 = 2052 元
  {
    transaction_id: "mock-txn-101",
    instrument_id: "mock-inst-etf",
    action: "buy",
    quantity: 1000,
    price_cents: 20_500, // 2.05 元（万分之一元刻度，与标的现价同值）
    fee_cents: 200,
    to_instrument_id: null,
    to_quantity: null,
    out_amount_cents: null,
    in_amount_cents: null,
  },
  // sell：基金金额权威（确认净额 30 元）——毛收入 30.3 元反算单价 1.2625 元
  {
    transaction_id: "mock-txn-102",
    instrument_id: "mock-inst-fund",
    action: "sell",
    quantity: 2400,
    price_cents: 12_625, // (3000 + 30) × 10000 ÷ 2400 反算
    fee_cents: 30,
    to_instrument_id: null,
    to_quantity: null,
    out_amount_cents: null,
    in_amount_cents: null,
  },
  // convert：两腿同记录（转出腿 = 公共标的地），行金额锚点为结转成本 198 元
  {
    transaction_id: "mock-txn-103",
    instrument_id: "mock-inst-fund",
    action: "convert",
    quantity: 1600, // 转出份额
    price_cents: null,
    fee_cents: 100,
    to_instrument_id: "mock-inst-etf",
    to_quantity: 97,
    out_amount_cents: 20_000,
    in_amount_cents: 19_900,
  },
  // split：带符号份额增量 Δ = +5（折算/送股形态），无现金腿
  {
    transaction_id: "mock-txn-104",
    instrument_id: "mock-inst-custom",
    action: "split",
    quantity: 5,
    price_cents: null,
    fee_cents: 0,
    to_instrument_id: null,
    to_quantity: null,
    out_amount_cents: null,
    in_amount_cents: null,
  },
  // dividend：现金分红，无数量单价；到账账户即账户端（mock-txn-105 → 工资卡）
  {
    transaction_id: "mock-txn-105",
    instrument_id: "mock-inst-hk",
    action: "dividend",
    quantity: null,
    price_cents: null,
    fee_cents: 0,
    to_instrument_id: null,
    to_quantity: null,
    out_amount_cents: null,
    in_amount_cents: null,
  },
];

// —— 交易（覆盖主列表四种通用 kind：expense / income / transfer / refund，ADR-0135；
// 投资五种 kind 见上方 fixtureSecurityTxns 块） ——

function txn(base: Omit<Transaction, keyof ReturnType<typeof refRow>>): Transaction {
  return { ...base, ...refRow() };
}

export const fixtureTransactions: Transaction[] = [
  txn({
    id: "mock-txn-001",
    kind: "expense",
    amount_cents: 2_500,
    currency_code: "CNY",
    amount_native_cents: 2_500,
    fx_rate_used: null,
    fx_rate_source: null,
    account_id: "mock-acc-ewallet",
    to_account_id: null,
    funding_account_id: null,
    category_id: "mock-cat-food",
    merchant_id: "mock-mch-meituan",
    policy_id: null,
    source: null,
    convert: null,
    refund_of_transaction_id: null,
    fundings: [],
    purchases: [],
    source_order_no: null,
    note: "午餐",
    date: "2026-01-05",
    created_at: "2026-01-05T12:00:00Z",
  }),
  txn({
    id: "mock-txn-002",
    kind: "income",
    amount_cents: 1_200_000,
    currency_code: "CNY",
    amount_native_cents: 1_200_000,
    fx_rate_used: null,
    fx_rate_source: null,
    account_id: "mock-acc-bank",
    to_account_id: null,
    funding_account_id: null,
    category_id: "mock-cat-salary",
    merchant_id: null,
    policy_id: null,
    source: null,
    convert: null,
    refund_of_transaction_id: null,
    fundings: [],
    purchases: [],
    source_order_no: null,
    note: null,
    date: "2026-01-10",
    created_at: "2026-01-10T09:00:00Z",
  }),
  txn({
    id: "mock-txn-003",
    kind: "transfer",
    amount_cents: 300_000,
    currency_code: "CNY",
    amount_native_cents: 300_000,
    fx_rate_used: null,
    fx_rate_source: null,
    account_id: "mock-acc-bank",
    to_account_id: "mock-acc-credit",
    funding_account_id: null,
    category_id: null,
    merchant_id: null,
    policy_id: null,
    source: null,
    convert: null,
    refund_of_transaction_id: null,
    fundings: [],
    purchases: [],
    source_order_no: null,
    note: "还卡账",
    date: "2026-01-25",
    created_at: "2026-01-25T20:00:00Z",
  }),
  txn({
    id: "mock-txn-004",
    kind: "refund",
    amount_cents: 1_500,
    currency_code: "CNY",
    amount_native_cents: 1_500,
    fx_rate_used: null,
    fx_rate_source: null,
    account_id: "mock-acc-ewallet",
    to_account_id: null,
    funding_account_id: null,
    category_id: "mock-cat-food",
    merchant_id: "mock-mch-meituan",
    policy_id: null,
    source: null,
    convert: null,
    refund_of_transaction_id: "mock-txn-001",
    fundings: [],
    purchases: [],
    source_order_no: null,
    note: "部分退款",
    date: "2026-01-06",
    created_at: "2026-01-06T10:00:00Z",
  }),
  // 投资五种 kind（与 fixtureSecurityTxns 一一对应；币种 = 账户币种 CNY，
  // amount_native_cents 不折算原币直记，ADR-0143 决策 1）
  txn({
    id: "mock-txn-101",
    kind: "buy",
    amount_cents: 205_200, // 1000 × 20_500 ÷ 10000 + 200（写路径同公式人算）
    currency_code: "CNY",
    amount_native_cents: 205_200,
    fx_rate_used: null,
    fx_rate_source: null,
    account_id: "mock-acc-invest",
    to_account_id: null,
    funding_account_id: "mock-acc-bank",
    category_id: null,
    merchant_id: null,
    policy_id: null,
    source: null,
    convert: null,
    refund_of_transaction_id: null,
    fundings: [],
    purchases: [],
    source_order_no: "ORD-20260203-001",
    note: "ETF 建仓",
    date: "2026-02-03",
    created_at: "2026-02-03T10:00:00Z",
  }),
  txn({
    id: "mock-txn-102",
    kind: "sell",
    amount_cents: 3_000, // 基金确认净额（金额权威）
    currency_code: "CNY",
    amount_native_cents: 3_000,
    fx_rate_used: null,
    fx_rate_source: null,
    account_id: "mock-acc-invest",
    to_account_id: null,
    funding_account_id: "mock-acc-bank", // 现金腿到账（结算账户 ?? 投资账户）
    category_id: null,
    merchant_id: null,
    policy_id: null,
    source: null,
    convert: null,
    refund_of_transaction_id: null,
    fundings: [],
    purchases: [],
    source_order_no: null,
    note: null,
    date: "2026-02-10",
    created_at: "2026-02-10T09:30:00Z",
  }),
  txn({
    id: "mock-txn-103",
    kind: "convert",
    amount_cents: 19_800, // 行金额锚点 = 结转成本（非确认单金额）
    currency_code: "CNY",
    amount_native_cents: 19_800,
    fx_rate_used: null,
    fx_rate_source: null,
    account_id: "mock-acc-invest",
    to_account_id: null,
    funding_account_id: null,
    category_id: null,
    merchant_id: null,
    policy_id: null,
    source: null,
    // 主列表 convert 扩展（读路径填充惯例）：与 fixtureSecurityTxns 两腿数据同源
    convert: {
      to_instrument_id: "mock-inst-etf",
      to_symbol: "159915",
      to_quantity: 97,
      out_amount_cents: 20_000,
      in_amount_cents: 19_900,
    },
    refund_of_transaction_id: null,
    fundings: [],
    purchases: [],
    source_order_no: null,
    note: null,
    date: "2026-02-15",
    created_at: "2026-02-15T14:00:00Z",
  }),
  txn({
    id: "mock-txn-104",
    kind: "split",
    amount_cents: 0, // 无现金腿恒 0
    currency_code: "CNY",
    amount_native_cents: 0,
    fx_rate_used: null,
    fx_rate_source: null,
    account_id: "mock-acc-invest",
    to_account_id: null,
    funding_account_id: null,
    category_id: null,
    merchant_id: null,
    policy_id: null,
    source: null,
    convert: null,
    refund_of_transaction_id: null,
    fundings: [],
    purchases: [],
    source_order_no: null,
    note: "黄金折算",
    date: "2026-02-20",
    created_at: "2026-02-20T11:00:00Z",
  }),
  txn({
    id: "mock-txn-105",
    kind: "dividend",
    amount_cents: 8_500, // 现金分红 85 元
    currency_code: "CNY",
    amount_native_cents: 8_500,
    fx_rate_used: null,
    fx_rate_source: null,
    account_id: "mock-acc-bank", // 账户端 = 到账账户（与 buy/sell 的投资账户语义区分）
    to_account_id: null,
    funding_account_id: null,
    category_id: null,
    merchant_id: null,
    policy_id: null,
    source: null,
    convert: null,
    refund_of_transaction_id: null,
    fundings: [],
    purchases: [],
    source_order_no: null,
    note: null,
    date: "2026-02-25",
    created_at: "2026-02-25T16:00:00Z",
  }),
];

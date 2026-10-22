import type {
  Account,
  Budget,
  Category,
  Currency,
  ExchangeRate,
  Instrument,
  Insurer,
  MarketPrice,
  Merchant,
  Transaction,
} from "@ledger/types";

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

// —— 汇率（issue #1972）：当期汇率表种子，少量货币对覆盖手动 / 同步两种
// 来源形态；rate 为 base → quote 计价（1 base = rate quote）。

export const fixtureExchangeRates: ExchangeRate[] = [
  {
    id: "mock-fx-cny-usd",
    base_code: "CNY",
    quote_code: "USD",
    rate: 0.1389,
    priced_at: "2026-01-15",
    source: "manual",
    updated_at: T0,
    version: 1,
    device_id: "mock",
  },
  {
    id: "mock-fx-usd-cny",
    base_code: "USD",
    quote_code: "CNY",
    rate: 7.2,
    priced_at: "2026-01-15",
    source: "ecb",
    updated_at: T0,
    version: 1,
    device_id: "mock",
  },
];

// —— 行情现价缓存（issue #1972）：种子行与 fixtureInstruments 有现价的五标的
// 一一对应（price_cents 同值，两表一致性）；019666 无现价行（手动建档未录价
// 形态）。priced_at 按各价格通道的真实形态取字面量：净值通道为净值日期、
// 行情/手动通道为数据日期。

export const fixtureMarketPrices: MarketPrice[] = [
  {
    id: "mock-mp-custom",
    instrument_id: "mock-inst-custom",
    price_cents: 55_000, // 5.50 元/克，与标的现价列同值
    currency_code: "CNY",
    priced_at: "2026-01-10",
    source: "manual",
    created_at: T0,
    updated_at: T0,
    version: 1,
    device_id: "mock",
  },
  {
    id: "mock-mp-etf",
    instrument_id: "mock-inst-etf",
    price_cents: 20_500, // 2.05 元
    currency_code: "CNY",
    priced_at: "2026-01-30",
    source: "eastmoney",
    created_at: T0,
    updated_at: T0,
    version: 1,
    device_id: "mock",
  },
  {
    id: "mock-mp-fund",
    instrument_id: "mock-inst-fund",
    price_cents: 12_345, // 1.2345 元净值（净值日期即 priced_at）
    currency_code: "CNY",
    priced_at: "2026-01-31",
    source: "eastmoney",
    created_at: T0,
    updated_at: T0,
    version: 1,
    device_id: "mock",
  },
  {
    id: "mock-mp-hk",
    instrument_id: "mock-inst-hk",
    price_cents: 385_000_000, // 38500.00 港元
    currency_code: "HKD",
    priced_at: "2026-01-30",
    source: "eastmoney",
    created_at: T0,
    updated_at: T0,
    version: 1,
    device_id: "mock",
  },
  {
    id: "mock-mp-sh",
    instrument_id: "mock-inst-sh",
    price_cents: 1_485_000_000, // 148500.00 元
    currency_code: "CNY",
    priced_at: "2026-01-30",
    source: "eastmoney",
    created_at: T0,
    updated_at: T0,
    version: 1,
    device_id: "mock",
  },
];

// —— 价格历史（issue #1972）：替身内部承载（真后端 PriceHistory 无 IPC 直读
// 面，由走势等聚合消费），手动报价的历史落点写入这里、现价落点的「最新点
// 映像」判定读这里。种子仅 GOLD 一行（手动通道标的 + 历史水位锚 2026-01-10）。

export interface MockPriceHistory {
  instrument_id: string;
  /** 交易日（ISO YYYY-MM-DD，行内主时点） */
  trade_date: string;
  /** ISO 周一日期（周采样自然键的第二维，与真后端 week_start 生成列同义） */
  week_start: string;
  price_cents: number;
  currency_code: string;
  source: string;
  updated_at: string;
  version: number;
  device_id: string;
}

export const fixturePriceHistory: MockPriceHistory[] = [
  {
    instrument_id: "mock-inst-custom",
    trade_date: "2026-01-10",
    week_start: "2026-01-05", // 2026-01-10 为周六，所在 ISO 周的周一
    price_cents: 55_000,
    currency_code: "CNY",
    source: "manual",
    updated_at: T0,
    version: 1,
    device_id: "mock",
  },
];

// —— 交易（覆盖主列表四种通用 kind：expense / income / transfer / refund，ADR-0135） ——

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
];

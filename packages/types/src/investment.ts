import type { Syncable } from "./common";

export type InstrumentType = "stock" | "fund" | "bond" | "etf" | "other";

export type MarketType = "sh" | "sz" | "hk" | "nasdaq" | "nyse" | "amex" | "unknown";

/**
 * 市场闭集镜像（ADR-0081）：显示标签在文案资源 investments.market.*（i18n，
 * ADR-0049）；美股三交易所（nasdaq/nyse/amex）UI 折叠显示「美股」，仅用于
 * 枚举标签翻译（enumLabel 闭集成员判定），筛选下拉不得展开三交易所选项
 * （见 MARKET_FILTER_TYPES）。
 */
export const MARKET_TYPES: MarketType[] = ["sh", "sz", "hk", "nasdaq", "nyse", "amex", "unknown"];

/** 标的页市场筛选下拉的选项闭集（ADR-0081）：后端筛选为精确匹配，美股三
 * 交易所不展开重复选项（UI 只显「美股」的折叠语义，见 MARKET_TYPES 注释）。 */
export const MARKET_FILTER_TYPES: MarketType[] = ["sh", "sz", "hk", "unknown"];

/**
 * 「添加投资标的」的录入通道（市场下拉，issue #697 / spec #690；六通道修订
 * issue #826）：通道标签而非存储市场——场外基金（fund）走按代码即拉、落
 * fund 类型恒 unknown 市场（ADR-0038）；美股（us）是三交易所的 UI 折叠，
 * 落库经候选遍历取精确归属（ADR-0081）；沪/深/港为显式市场通道；自定义
 * 标的（custom）零查询直接建档、落 unknown 市场（与类型白名单的「其他」
 * 区分——那是 instrument_type 成员，这是录入通道）。
 */
export type AddInstrumentChannel = "sh" | "sz" | "hk" | "us" | "fund" | "custom";

/** 标的类型闭集；显示标签在文案资源 investments.type.*（i18n，ADR-0049） */
export const INSTRUMENT_TYPES: InstrumentType[] = ["stock", "fund", "bond", "etf", "other"];

/** 字典条目来源（自建标的，ADR-0036）：与价格侧 source 同词表但语义正交，随行终身不变；
 * 唯一功能消费是删除准入（仅手动行可删），不进用户可见列表（issue #1189）。 */
export type InstrumentSource = "eastmoney" | "manual";

/** 价格列刻度（ADR-0038）：投资域 price_cents 为万分之一元（元 × 10000），金额列仍是整数分 */
export interface Instrument extends Syncable {
  id: string;
  symbol: string;
  type: InstrumentType;
  name: string | null;
  currency_code: string;
  market: MarketType;
  created_at: string;
  /** 字典条目来源（同步/手动，ADR-0036）；存量行由 #293 迁移回填为同步 */
  source: InstrumentSource;
  price_cents: number | null;
  /** 是否持有该标的（有当前持仓，派生自 security_lots） */
  invested: boolean;
  /** 价格写入通道（后端派生事实，不落库，issue #1060）：单标的走势放行与
   * 录价入口的唯一判定来源，前端不再自行按类型与市场推断 */
  price_channel: InstrumentPriceChannel;
}

/** 价格写入通道（后端派生事实，issue #1060）：quote 行情 / fund_nav 净值 /
 * constant 恒定价格（ADR-0126）/ manual 手动报价 / none 无来源；判定单点在
 * 后端（`derive_price_channel`），与标的信息同步的通道分区同源。 */
export type InstrumentPriceChannel = "quote" | "fund_nav" | "constant" | "manual" | "none";

/** 价格通道闭集镜像（判定单点在后端，此处仅供前端按序渲染 i18n 标签）；
 * 显示标签在文案资源 investments.priceChannel.*（i18n，ADR-0049）。 */
export const INSTRUMENT_PRICE_CHANNELS: InstrumentPriceChannel[] = [
  "quote",
  "fund_nav",
  "constant",
  "manual",
  "none",
];

/**
 * 价格过期检查结果（issue #1190）：打开投资页时的本地水位检查（零网络请求）——
 * 有价格通道标的的现价水位（行情采集时刻 / 净值日期）超出阈值，或持仓标的没有
 * 现价时的计数；阈值随结果透出，提示文案不另抄一份天数。计数为 0 即不提示。
 */
export interface PriceStaleness {
  stale_count: number;
  threshold_days: number;
}

/**
 * `investment_overview` 命令返回的投资概览读数（spec #1532 / issue #1536）：
 * 投资页「概览」页签的唯一取数接缝——全页折全局默认币种单值，前端只装配数值，
 * 不做任何折算或分组（口径与折算单点在后端投资域；与持仓视图「按账户币种分组、
 * 不跨币种合并」的分工见 ADR-0131）。金额单位：分。
 */
export interface InvestmentOverview {
  /** 折算基准币种（全局默认币种）——本页全部金额的币种标注来源 */
  native_currency: string;
  /** 可投资资产合计 = 投资账户现金腿 + 持仓市值腿 */
  investable_assets_cents: number;
  /** 可投资资产·投资账户现金腿（排除隐藏账户） */
  investment_cash_cents: number;
  /** 可投资资产·持仓市值腿（排除隐藏账户、缺价持仓跳过） */
  holdings_market_value_cents: number;
  /** 投资合计·总市值 = Σ 折本位币持仓市值（与持仓市值腿同一聚合，缺价持仓跳过） */
  total_market_value_cents: number;
  /** 投资合计·持仓收益（未实现盈亏，折本位币，缺价持仓跳过） */
  unrealized_pnl_cents: number;
  /** 投资合计·累计收益 = 持仓收益 + 已实现盈亏 + 累计分红（均折本位币） */
  cumulative_pnl_cents: number;
  /** 未计入合计的持仓数：缺现价（或缺价格币→账户币汇率）按空值语义跳过的行数 */
  missing_price_holding_count: number;
  /** 账本内是否存在未删除的投资账户：否时展示「还没有投资账户」引导句 */
  has_investment_account: boolean;
}

export interface InstrumentInput {
  symbol: string;
  type: InstrumentType;
  name?: string | null;
  currency_code: string;
  market?: MarketType | null;
}

/** 标的列表查询过滤条件（服务端分页 + 搜索） */
export interface InstrumentListFilter {
  /** 对 symbol / name 的大小写不敏感子串匹配 */
  search?: string | null;
  /** 交易市场精确匹配（sh / sz / hk / unknown） */
  market?: MarketType | null;
  /** 标的类型过滤：同码异类型消歧用（issue #294） */
  type?: InstrumentType | null;
  /** 只看持仓标的：仅返回有当前持仓的标的 */
  only_invested?: boolean | null;
  /** 页码，从 1 开始，默认 1 */
  page?: number;
  /** 每页条数，默认 50，上限 500 */
  page_size?: number;
}

/** 标的列表分页结果 */
export interface InstrumentListResult {
  items: Instrument[];
  /** 满足过滤条件的总条数（用于分页条） */
  total: number;
}

/** 手动报价入参（issue #291 / ADR-0036）：标的 id + 日期（ISO）+ 价格（万分之一元，价格刻度） */
export interface ManualPriceInput {
  instrument_id: string;
  /** 报价对应的交易日（ISO YYYY-MM-DD） */
  date: string;
  /** 单价（万分之一元，ADR-0038 价格刻度） */
  price_cents: number;
}

/** 手动报价结果：两个落点各自的实际写入情况（回填旧价时 current_price_written 为 false） */
export interface ManualPriceResult {
  history_written: boolean;
  current_price_written: boolean;
}

/** 按代码即拉添加基金的结果（issue #301 / ADR-0038）：标的行落库 + 现价写入状态 */
export interface AddFundResult {
  instrument_id: string;
  symbol: string;
  /** 数据源权威名称（已回填标的行） */
  name: string;
  /** 已弃用：基金分类在替代源无来源，恒为空串（ADR-0130 决策 8；字段保留为契约的一部分，展示层可不渲染） */
  fund_class: string;
  /** 最新单位净值（万分之一元，ADR-0038 价格刻度）；未取到为 null */
  nav_cents: number | null;
  /** 净值日期（ISO 日期）；未取到为 null */
  nav_date: string | null;
  /** 是否落了现价缓存（后端据此判定是否广播价格失效信号） */
  price_written: boolean;
}

/** 「添加投资标的」股票侧（沪/深/港/美股通道）按代码添加的结果（issue #697 /
 * ADR-0081）：标的行落库 + 识别回显投影；场外基金通道返回 AddFundResult。 */
export interface AddStockInstrumentResult {
  instrument_id: string;
  /** 归一化代码（港股左补零至 5 位、美股大写） */
  symbol: string;
  /** 数据源权威名称（已回填标的行） */
  name: string;
  /** 自动识别的类型（stock / etf） */
  type: InstrumentType;
  /** 精确市场（美股为遍历命中的交易所归属） */
  market: MarketType;
  currency_code: string;
  /** 最新价（万分之一元，ADR-0038 价格刻度）；停牌/无有效报价为 null */
  price_cents: number | null;
  /** 价格日期（ISO 日期）；无有效时间戳为 null */
  price_date: string | null;
  /** 是否落了现价缓存（后端据此判定是否广播价格失效信号） */
  price_written: boolean;
}

/** 交易买卖明细（issue #180）：一笔 buy/sell 交易在扩展表中的投影（核心交易行
 * 不含投资字段），供投资表单编辑模式回填标的/数量/价格/费用。`symbol`/`instrument_name`
 * 为 JOIN 标的表带出的展示字段，保证回填后标的选择框直接显示标的而非裸 id；
 * `instrument_type` 驱动表单录入形态切换（基金 = 金额 + 份额必填、单价反算，issue #302）。 */
export interface TransactionTrade {
  instrument_id: string;
  symbol: string;
  instrument_name: string | null;
  instrument_type: InstrumentType;
  quantity: number;
  price_cents: number;
  fee_cents: number | null;
}

/** 基金转换两腿明细（ADR-0099 / issue #979）：一笔 convert 交易在扩展表中的投影
 * （两腿同记录），供转换表单编辑模式回填「A → B」全量信息。两侧金额为确认单权威
 * （`out_amount_cents` / `in_amount_cents`），两侧单价由表单按金额 ÷ 份额反算展示
 * （万分之一元，ADR-0038），故投影不冗余携带；`carried_cost_cents` 是行金额锚点
 * （服务端按 FIFO 消耗算定的结转成本，非确认单金额）。 */
export interface TransactionConvert {
  out_instrument_id: string;
  out_symbol: string;
  out_instrument_name: string | null;
  out_quantity: number;
  out_amount_cents: number;
  in_instrument_id: string;
  in_symbol: string;
  in_instrument_name: string | null;
  in_quantity: number;
  in_amount_cents: number;
  fee_cents: number;
  carried_cost_cents: number;
  currency_code: string;
}

/** 份额调整明细（ADR-0106 / issue #1052）：一笔 split 交易在 `security_transactions`
 * 扩展表中的投影（`quantity` = **带符号**份额增量 Δ）；供交易列表「只读详情」呈现
 * 标的与份额变动。`instrument_name` 为 JOIN `instruments` 带出的展示字段（可空）。 */
export interface TransactionSplit {
  instrument_id: string;
  symbol: string;
  instrument_name: string | null;
  /** 带符号份额增量 Δ：`+` = 折算 / 结转 / 送股，`-` = 缩股。 */
  quantity: number;
}

export interface Holding {
  id: string;
  account_id: string;
  instrument_id: string;
  quantity: number;
  cost_basis_cents: number;
  cost_currency_code: string;
  latest_price_cents: number | null;
  latest_price_currency_code: string | null;
  /** 净值日期：基金现价（= 最新公布单位净值）携带，现价对应哪天的净值；股票恒 null */
  latest_nav_date: string | null;
  market_value_cents: number | null;
  unrealized_pnl_cents: number | null;
  updated_at: string;
}

export interface MarketPrice {
  id: string;
  instrument_id: string;
  price_cents: number;
  currency_code: string;
  priced_at: string;
  source: string | null;
  created_at: string;
  updated_at: string;
  version: number;
  device_id: string;
}

export interface MarketPriceInput {
  instrument_id: string;
  price_cents: number;
  currency_code: string;
  priced_at: string;
  source?: string | null;
}

/** 按年分组的已实现收益行（ADR-0129）：两腿（已实现盈亏 / 现金分红）并列，
 * realized_gain_cents = 两腿之和（词汇表「已实现收益（RealizedGain）」） */
export interface YearPnl {
  year: string;
  currency_code: string;
  realized_pnl_cents: number;
  dividend_cents: number;
  realized_gain_cents: number;
}

/** 按账户分组的已实现收益行（ADR-0129）：列口径同 YearPnl，按账户聚合 */
export interface AccountPnl {
  account_id: string;
  account_name: string;
  currency_code: string;
  realized_pnl_cents: number;
  dividend_cents: number;
  realized_gain_cents: number;
}

export interface InstrumentPnl {
  instrument_id: string;
  symbol: string;
  name: string | null;
  currency_code: string;
  realized_pnl_cents: number;
}

export interface PnlFilter {
  account_id?: string | null;
  instrument_id?: string | null;
}

/** 按币种分组的已实现盈亏小计（ADR-0107 决策 6）：匹配行币种口径，不做跨币种折算 */
export interface CurrencyPnl {
  currency_code: string;
  realized_pnl_cents: number;
}

/**
 * 按币种分组的累计收益小计（issue #1077 / 词汇表「累计收益（CumulativePnl）」）：
 * 未实现盈亏（Holding）+ 已实现盈亏（RealizedPnl）+ 累计分红（现金分红 dividend）
 * 三腿相加，按账户币种独立成组、不做跨币种折算（同 ADR-0107 决策 6 的持仓合计口径）。
 * 缺价 / 缺汇率持仓的未实现腿为空值 → 不计入且不以零计入（Holding 侧语义）；
 * 三腿皆空的币种不出现该组。
 */
export interface CurrencyCumulativePnl {
  currency_code: string;
  cumulative_pnl_cents: number;
}

/** 已实现盈亏汇总（ADR-0107）：盈亏页三视图（按年/按账户/按标的）+ 按币种分组总数；
 * 逐匹配「卖出明细」已退役（决策 1），明细数据不再随本投影返回 */
export interface RealizedPnlSummary {
  total: CurrencyPnl[];
  by_year: YearPnl[];
  by_account: AccountPnl[];
  by_instrument: InstrumentPnl[];
}

/** 走势查询区间：可选起止 ISO 日期，缺省表示该侧不设界（"全部"区间） */
export interface TrendRange {
  start_date?: string | null;
  end_date?: string | null;
}

/** 走势空态补全状态三态闭集（与后端 serde snake_case 字面量一一对应，ADR-0122
 * 决策 5 / issue #1377）：running = 补全中；retry_pending = 补全失败待重试；
 * no_data = 无数据（确实没有可采的历史序列）。 */
export type TrendBackfillState = "running" | "retry_pending" | "no_data";

/** 走势空态的补全状态（读投影只增字段）：仅当采样点为空且标的有价格写入
 * 通道而磁盘上没有任何历史序列时携带；其余场景缺省不序列化（旧消费方零破坏）。 */
export interface TrendBackfillStatus {
  state: TrendBackfillState;
  /** 在途轮次已完成的标的数（仅 running 且有在途轮次时携带）。 */
  done?: number;
  /** 在途轮次的队列总长（仅 running 且有在途轮次时携带）。 */
  total?: number;
}

/** 单标的走势采样点：周采样交易日 + 收盘价（报价币种万分之一元，价格刻度见上） */
export interface PriceTrendPoint {
  date: string;
  price_cents: number;
  currency_code: string;
}

/** 单标的走势：区间裁剪后的周采样点序列（从首个有效点开始） */
export interface InstrumentPriceTrend {
  instrument_id: string;
  points: PriceTrendPoint[];
  /** 补全状态（issue #1377）：仅空采样点且有通道无历史序列时携带。 */
  backfill?: TrendBackfillStatus;
}

/** 组合走势采样点：该周组合总市值（分，本位币） */
export interface PortfolioTrendPoint {
  date: string;
  market_value_cents: number;
}

/** 投资资产走势：组合市值周点曲线；points 为空即无历史数据的空态 */
export interface PortfolioValueTrend {
  /** 折算基准（本位币） */
  currency_code: string;
  points: PortfolioTrendPoint[];
  /** 补全状态（issue #1377）：仅空采样点且库内存在有通道无历史序列的标的时携带。 */
  backfill?: TrendBackfillStatus;
}

/** 资金加权收益率查询区间（issue #1195 / ADR-0115）：可选起止 ISO 日期，缺省
 * 表示该侧不设界（自首笔流水起算、截至今日现值）；区间开始存量持仓按区间首日
 * 市值折为期初投入（收益率的输入假设，不改账务） */
export interface MwrRange {
  start_date?: string | null;
  end_date?: string | null;
}

/** 收益率口径（issue #1343 / ADR-0115 修订）：annualized = 年化内部收益率
 * （XIRR，实际天数 / 365）；cumulative = 未年化收益率（累计收益 ÷ 累计投入，
 * 用于含期初存量、真实建仓时点未知的标的）。两者**不可互算**，展示层按
 * basis 标注口径 */
export type MwrBasis = "annualized" | "cumulative";

/** 单标的资金加权收益率行（持仓页每行，账户 × 标的粒度）：basis 标口径，rate 为
 * 该口径下的收益率（小数，0.1234 = 12.34%）；缺价跳过的行不出现（前端渲染
 * 「-」），无解（或未年化口径投入为零）的行 rate 为 null（显式标注无法计算，
 * 不猜解） */
export interface InstrumentMwr {
  account_id: string;
  instrument_id: string;
  /** 计价币种 = 账户币种（组内不跨币种折算） */
  currency_code: string;
  /** 本行收益率的口径（#1343） */
  basis: MwrBasis;
  rate: number | null;
}

/** 账户级资金加权收益率行（盈亏页账户粒度）：只覆盖投资账户；basis 标本行
 * 口径——名下合集含期初存量标的时整项给未年化（#1346），否则年化 */
export interface AccountMwr {
  account_id: string;
  account_name: string;
  currency_code: string;
  /** 本行收益率的口径（#1346） */
  basis: MwrBasis;
  rate: number | null;
}

/** 全账级按币种分组的资金加权收益率行：不做跨币种折算；basis 同 AccountMwr
 * （该币种合集含期初存量标的时整项给未年化，#1346） */
export interface CurrencyMwr {
  currency_code: string;
  /** 本行收益率的口径（#1346） */
  basis: MwrBasis;
  rate: number | null;
}

/** 资金加权收益率汇总（ADR-0115 / issue #1195）：三个消费面（持仓页单标的 /
 * 盈亏页账户级与全账级）共用的只读投影 */
export interface MoneyWeightedReturnSummary {
  by_instrument: InstrumentMwr[];
  by_account: AccountMwr[];
  total: CurrencyMwr[];
}

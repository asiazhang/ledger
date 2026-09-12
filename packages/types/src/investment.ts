import type { Syncable } from './common'

export type InstrumentType = 'stock' | 'fund' | 'bond' | 'etf' | 'other'

export type MarketType = 'sh' | 'sz' | 'hk' | 'nasdaq' | 'nyse' | 'amex' | 'unknown'

/**
 * 市场闭集镜像（ADR-0081）：显示标签在文案资源 investments.market.*（i18n，
 * ADR-0049）；美股三交易所（nasdaq/nyse/amex）UI 折叠显示「美股」，仅用于
 * 枚举标签翻译（enumLabel 闭集成员判定），筛选下拉不得展开三交易所选项
 * （见 MARKET_FILTER_TYPES）。
 */
export const MARKET_TYPES: MarketType[] = [
  'sh',
  'sz',
  'hk',
  'nasdaq',
  'nyse',
  'amex',
  'unknown',
]

/** 标的页市场筛选下拉的选项闭集（ADR-0081）：后端筛选为精确匹配，美股三
 * 交易所不展开重复选项（UI 只显「美股」的折叠语义，见 MARKET_TYPES 注释）。 */
export const MARKET_FILTER_TYPES: MarketType[] = ['sh', 'sz', 'hk', 'unknown']

/**
 * 「添加投资标的」的录入通道（市场下拉，issue #697 / spec #690；六通道修订
 * issue #826）：通道标签而非存储市场——场外基金（fund）走按代码即拉、落
 * fund 类型恒 unknown 市场（ADR-0038）；美股（us）是三交易所的 UI 折叠，
 * 落库经候选遍历取精确归属（ADR-0081）；沪/深/港为显式市场通道；自定义
 * 标的（custom）零查询直接建档、落 unknown 市场（与类型白名单的「其他」
 * 区分——那是 instrument_type 成员，这是录入通道）。
 */
export type AddInstrumentChannel = 'sh' | 'sz' | 'hk' | 'us' | 'fund' | 'custom'

/** 标的类型闭集；显示标签在文案资源 investments.type.*（i18n，ADR-0049） */
export const INSTRUMENT_TYPES: InstrumentType[] = ['stock', 'fund', 'bond', 'etf', 'other']

/** 字典条目来源（自建标的，ADR-0036）：与价格侧 source 同词表但语义正交，随行终身不变 */
export type InstrumentSource = 'eastmoney' | 'manual'

/** 字典来源闭集；显示标签在文案资源 investments.source.*（i18n，ADR-0049） */
export const INSTRUMENT_SOURCES: InstrumentSource[] = ['eastmoney', 'manual']

/** 价格列刻度（ADR-0038）：投资域 price_cents 为万分之一元（元 × 10000），金额列仍是整数分 */
export interface Instrument extends Syncable {
  id: string
  symbol: string
  type: InstrumentType
  name: string | null
  currency_code: string
  market: MarketType
  created_at: string
  /** 字典条目来源（同步/手动，ADR-0036）；存量行由 #293 迁移回填为同步 */
  source: InstrumentSource
  price_cents: number | null
  /** 是否持有该标的（有当前持仓，派生自 security_lots） */
  invested: boolean
  /** 价格写入通道（后端派生事实，不落库，issue #1060）：单标的走势放行与
   * 录价入口的唯一判定来源，前端不再自行按类型与市场推断 */
  price_channel: InstrumentPriceChannel
}

/** 价格写入通道（后端派生事实，issue #1060）：quote 行情 / fund_nav 净值 /
 * manual 手动报价 / none 无来源；判定单点在后端（`derive_price_channel`），
 * 与标的信息同步的通道分区同源。 */
export type InstrumentPriceChannel = 'quote' | 'fund_nav' | 'manual' | 'none'

export interface InstrumentInput {
  symbol: string
  type: InstrumentType
  name?: string | null
  currency_code: string
  market?: MarketType | null
}

/** 标的列表查询过滤条件（服务端分页 + 搜索） */
export interface InstrumentListFilter {
  /** 对 symbol / name 的大小写不敏感子串匹配 */
  search?: string | null
  /** 交易市场精确匹配（sh / sz / hk / unknown） */
  market?: MarketType | null
  /** 标的类型过滤：同码异类型消歧用（issue #294） */
  type?: InstrumentType | null
  /** 只看持仓标的：仅返回有当前持仓的标的 */
  only_invested?: boolean | null
  /** 页码，从 1 开始，默认 1 */
  page?: number
  /** 每页条数，默认 50，上限 500 */
  page_size?: number
}

/** 标的列表分页结果 */
export interface InstrumentListResult {
  items: Instrument[]
  /** 满足过滤条件的总条数（用于分页条） */
  total: number
}

/** 手动报价入参（issue #291 / ADR-0036）：标的 id + 日期（ISO）+ 价格（万分之一元，价格刻度） */
export interface ManualPriceInput {
  instrument_id: string
  /** 报价对应的交易日（ISO YYYY-MM-DD） */
  date: string
  /** 单价（万分之一元，ADR-0038 价格刻度） */
  price_cents: number
}

/** 手动报价结果：两个落点各自的实际写入情况（回填旧价时 current_price_written 为 false） */
export interface ManualPriceResult {
  history_written: boolean
  current_price_written: boolean
}

/** 按代码即拉添加基金的结果（issue #301 / ADR-0038）：标的行落库 + 现价写入状态 */
export interface AddFundResult {
  instrument_id: string
  symbol: string
  /** 东财权威名称（已回填标的行） */
  name: string
  /** 东财基金分类（如「混合型-灵活」），展示透传，不落库 */
  fund_class: string
  /** 最新单位净值（万分之一元，ADR-0038 价格刻度）；未取到为 null */
  nav_cents: number | null
  /** 净值日期（ISO 日期）；未取到为 null */
  nav_date: string | null
  /** 是否落了现价缓存（后端据此判定是否广播价格失效信号） */
  price_written: boolean
}

/** 「添加投资标的」股票侧（沪/深/港/美股通道）按代码添加的结果（issue #697 /
 * ADR-0081）：标的行落库 + 识别回显投影；场外基金通道返回 AddFundResult。 */
export interface AddStockInstrumentResult {
  instrument_id: string
  /** 归一化代码（港股左补零至 5 位、美股大写） */
  symbol: string
  /** 东财权威名称（已回填标的行） */
  name: string
  /** 自动识别的类型（stock / etf） */
  type: InstrumentType
  /** 精确市场（美股为遍历命中的交易所归属） */
  market: MarketType
  currency_code: string
  /** 最新价（万分之一元，ADR-0038 价格刻度）；停牌/无有效报价为 null */
  price_cents: number | null
  /** 价格日期（ISO 日期）；无有效时间戳为 null */
  price_date: string | null
  /** 是否落了现价缓存（后端据此判定是否广播价格失效信号） */
  price_written: boolean
}

/** 交易买卖明细（issue #180）：一笔 buy/sell 交易在扩展表中的投影（核心交易行
 * 不含投资字段），供投资表单编辑模式回填标的/数量/价格/费用。`symbol`/`instrument_name`
 * 为 JOIN 标的表带出的展示字段，保证回填后标的选择框直接显示标的而非裸 id；
 * `instrument_type` 驱动表单录入形态切换（基金 = 金额 + 份额必填、单价反算，issue #302）。 */
export interface TransactionTrade {
  instrument_id: string
  symbol: string
  instrument_name: string | null
  instrument_type: InstrumentType
  quantity: number
  price_cents: number
  fee_cents: number | null
}

/** 基金转换两腿明细（ADR-0099 / issue #979）：一笔 convert 交易在扩展表中的投影
 * （两腿同记录），供转换表单编辑模式回填「A → B」全量信息。两侧金额为确认单权威
 * （`out_amount_cents` / `in_amount_cents`），两侧单价由表单按金额 ÷ 份额反算展示
 * （万分之一元，ADR-0038），故投影不冗余携带；`carried_cost_cents` 是行金额锚点
 * （服务端按 FIFO 消耗算定的结转成本，非确认单金额）。 */
export interface TransactionConvert {
  out_instrument_id: string
  out_symbol: string
  out_instrument_name: string | null
  out_quantity: number
  out_amount_cents: number
  in_instrument_id: string
  in_symbol: string
  in_instrument_name: string | null
  in_quantity: number
  in_amount_cents: number
  fee_cents: number
  carried_cost_cents: number
  currency_code: string
}

/** 份额调整明细（ADR-0106 / issue #1052）：一笔 split 交易在 `security_transactions`
 * 扩展表中的投影（`quantity` = **带符号**份额增量 Δ）；供交易列表「只读详情」呈现
 * 标的与份额变动。`instrument_name` 为 JOIN `instruments` 带出的展示字段（可空）。 */
export interface TransactionSplit {
  instrument_id: string
  symbol: string
  instrument_name: string | null
  /** 带符号份额增量 Δ：`+` = 折算 / 结转 / 送股，`-` = 缩股。 */
  quantity: number
}

export interface Holding {
  id: string
  account_id: string
  instrument_id: string
  quantity: number
  cost_basis_cents: number
  cost_currency_code: string
  latest_price_cents: number | null
  latest_price_currency_code: string | null
  /** 净值日期：基金现价（= 最新公布单位净值）携带，现价对应哪天的净值；股票恒 null */
  latest_nav_date: string | null
  market_value_cents: number | null
  unrealized_pnl_cents: number | null
  updated_at: string
}

export interface MarketPrice {
  id: string
  instrument_id: string
  price_cents: number
  currency_code: string
  priced_at: string
  source: string | null
  created_at: string
  updated_at: string
  version: number
  device_id: string
}

export interface MarketPriceInput {
  instrument_id: string
  price_cents: number
  currency_code: string
  priced_at: string
  source?: string | null
}

export interface YearPnl {
  year: string
  currency_code: string
  realized_pnl_cents: number
}

export interface AccountPnl {
  account_id: string
  account_name: string
  currency_code: string
  realized_pnl_cents: number
}

export interface InstrumentPnl {
  instrument_id: string
  symbol: string
  name: string | null
  currency_code: string
  realized_pnl_cents: number
}

export interface PnlFilter {
  account_id?: string | null
  instrument_id?: string | null
}

/** 按币种分组的已实现盈亏小计（ADR-0107 决策 6）：匹配行币种口径，不做跨币种折算 */
export interface CurrencyPnl {
  currency_code: string
  realized_pnl_cents: number
}

/**
 * 按币种分组的累计收益小计（issue #1077 / 词汇表「累计收益（CumulativePnl）」）：
 * 未实现盈亏（Holding）+ 已实现盈亏（RealizedPnl）+ 累计分红（现金分红 dividend）
 * 三腿相加，按账户币种独立成组、不做跨币种折算（同 ADR-0107 决策 6 的持仓合计口径）。
 * 缺价 / 缺汇率持仓的未实现腿为空值 → 不计入且不以零计入（Holding 侧语义）；
 * 三腿皆空的币种不出现该组。
 */
export interface CurrencyCumulativePnl {
  currency_code: string
  cumulative_pnl_cents: number
}

/** 已实现盈亏汇总（ADR-0107）：盈亏页三视图（按年/按账户/按标的）+ 按币种分组总数；
 * 逐匹配「卖出明细」已退役（决策 1），明细数据不再随本投影返回 */
export interface RealizedPnlSummary {
  total: CurrencyPnl[]
  by_year: YearPnl[]
  by_account: AccountPnl[]
  by_instrument: InstrumentPnl[]
}

/** 走势查询区间：可选起止 ISO 日期，缺省表示该侧不设界（"全部"区间） */
export interface TrendRange {
  start_date?: string | null
  end_date?: string | null
}

/** 单标的走势采样点：周采样交易日 + 收盘价（报价币种万分之一元，价格刻度见上） */
export interface PriceTrendPoint {
  date: string
  price_cents: number
  currency_code: string
}

/** 单标的走势：区间裁剪后的周采样点序列（从首个有效点开始） */
export interface InstrumentPriceTrend {
  instrument_id: string
  points: PriceTrendPoint[]
}

/** 组合走势采样点：该周组合总市值（分，本位币） */
export interface PortfolioTrendPoint {
  date: string
  market_value_cents: number
}

/** 投资资产走势：组合市值周点曲线；points 为空即无历史数据的空态 */
export interface PortfolioValueTrend {
  /** 折算基准（本位币） */
  currency_code: string
  points: PortfolioTrendPoint[]
}

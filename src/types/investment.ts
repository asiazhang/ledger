import type { Syncable } from './common'

export type InstrumentType = 'stock' | 'fund' | 'bond' | 'etf' | 'other'

export type MarketType = 'sh' | 'sz' | 'hk' | 'unknown'

export const MARKET_TYPE_LABELS: Record<MarketType, string> = {
  sh: '沪市',
  sz: '深市',
  hk: '港股',
  unknown: '未知',
}

export const INSTRUMENT_TYPE_LABELS: Record<InstrumentType, string> = {
  stock: '股票',
  fund: '基金',
  bond: '债券',
  etf: 'ETF',
  other: '其他',
}

export interface Instrument extends Syncable {
  id: string
  symbol: string
  type: InstrumentType
  name: string | null
  currency_code: string
  market: MarketType
  created_at: string
  price_cents: number | null
  /** 是否持有该标的（有当前持仓，派生自 security_lots） */
  invested: boolean
}

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

export interface Holding {
  id: string
  account_id: string
  instrument_id: string
  quantity: number
  cost_basis_cents: number
  cost_currency_code: string
  latest_price_cents: number | null
  latest_price_currency_code: string | null
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
  realized_pnl_cents: number
}

export interface AccountPnl {
  account_id: string
  account_name: string
  realized_pnl_cents: number
}

export interface InstrumentPnl {
  instrument_id: string
  symbol: string
  name: string | null
  realized_pnl_cents: number
}

export interface PnlDetail {
  id: string
  sell_date: string
  account_id: string
  account_name: string
  instrument_id: string
  instrument_symbol: string
  instrument_name: string | null
  quantity: number
  cost_per_unit_cents: number
  realized_pnl_cents: number
  currency_code: string
}

export interface PnlFilter {
  account_id?: string | null
  instrument_id?: string | null
}

export interface RealizedPnlSummary {
  total_realized_pnl_cents: number
  by_year: YearPnl[]
  by_account: AccountPnl[]
  by_instrument: InstrumentPnl[]
  details: PnlDetail[]
}

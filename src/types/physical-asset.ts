import type { Syncable } from './common'

/**
 * 实物资产（PhysicalAsset）领域类型（issue #466 / ADR-0064）：大件实物的
 * 估值档案（单列小域，先例物品/保单），与物品域按「要不要跟踪市值」互斥分家。
 * 估值全手动、只追加不改写：每次估值落一条历史行，当前估值 = 最新一条；
 * 金额一律整数分；当前估值折本位币走 Amount 接缝（当期汇率）。
 */

/** 实物资产生命周期状态：在持 / 已处置。 */
export type PhysicalAssetStatus = 'holding' | 'disposed'

/** 实物资产实体（读模型，全字段，对应后端 `physical_asset::PhysicalAsset`）。 */
export interface PhysicalAsset extends Syncable {
  id: string
  /** 资产名称（建档必填）。 */
  name: string
  /** 购买日期（可空；YYYY-MM-DD）。 */
  purchase_date: string | null
  /** 购买价（可空，整数分；纯记录，不进任何金额口径）。 */
  purchase_price_cents: number | null
  /** 购买价币种（与购买价成对：购买价存在时必填）。 */
  purchase_currency_code: string | null
  /** 生命周期状态（在持/已处置）。 */
  status: PhysicalAssetStatus
  /** 处置日期（仅 disposed；YYYY-MM-DD；处置必填）。 */
  disposal_date: string | null
  /** 处置价（可空，整数分；纯记录）。 */
  disposal_price_cents: number | null
  /** 处置价币种（与处置价成对）。 */
  disposal_currency_code: string | null
  created_at: string
  /** 当前估值（整数分）= 最新一条估值历史行金额。 */
  current_valuation_cents: number
  /** 当前估值币种。 */
  current_valuation_currency_code: string
  /** 当前估值日期（YYYY-MM-DD）。 */
  current_valuation_date: string
  /** 当前估值折本位币（整数分，当期汇率）：仅**在持**行有值，已处置行为 null。 */
  current_valuation_native_cents: number | null
  /** 本位币币种代码（折算基准）。 */
  native_currency: string
}

/** 建档入参（对应后端 `physical_asset::PhysicalAssetInput`，issue #466 T1）。
 *  名称必填、当前估值必填（即第一条估值历史行）、购买信息可选。 */
export interface PhysicalAssetInput {
  name: string
  /** 购买日期（可空；YYYY-MM-DD）。 */
  purchase_date?: string | null
  /** 购买价（可空，整数分）。 */
  purchase_price_cents?: number | null
  /** 购买价币种（购买价存在时必填）。 */
  purchase_currency_code?: string | null
  /** 当前估值（整数分；必填——缺失后端显式报错）。 */
  initial_valuation_cents?: number | null
  /** 当前估值币种（必填；前端预选默认币种）。 */
  initial_valuation_currency_code?: string | null
  /** 当前估值日期（可空 = 今天；YYYY-MM-DD）。 */
  initial_valuation_date?: string | null
}

/** 编辑档案入参（对应后端 `physical_asset::PhysicalAssetUpdateInput`，issue #467 T2）：
 *  仅名称与购买信息——估值不出现在编辑表单，只能经「更新估值」变更。 */
export interface PhysicalAssetUpdateInput {
  name: string
  /** 购买日期（可空；YYYY-MM-DD）。 */
  purchase_date?: string | null
  /** 购买价（可空，整数分）。 */
  purchase_price_cents?: number | null
  /** 购买价币种（购买价存在时必填）。 */
  purchase_currency_code?: string | null
}

/** 更新估值入参（对应后端 `physical_asset::PhysicalAssetValuationInput`，
 *  issue #467 T2）：每次调用追加一条估值历史行，当前估值变为最新一条。 */
export interface PhysicalAssetValuationInput {
  /** 估值金额（整数分；必填——缺失后端显式报错）。 */
  amount_cents?: number | null
  /** 估值币种（必填；前端预选当前估值币种）。 */
  currency_code?: string | null
  /** 估值日期（可空 = 今天；YYYY-MM-DD；可补过去，拒绝未来）。 */
  valuation_date?: string | null
}

/** 列表返回（对应后端 `physical_asset::PhysicalAssetList`）：
 *  资产行 + **在持**估值合计（口径与筛选无关——「家底合计」恒指在持资产）。 */
export interface PhysicalAssetList {
  assets: PhysicalAsset[]
  holding_total_native_cents: number
  native_currency: string
}

import type { Syncable } from './common'

/**
 * 物品（Item）领域类型（issue #116）：独立领域实体，与投资标的（Instrument）
 * 严格区分（CONTEXT.md `Item` 条目 / ADR-0014）。金额沿用整数分 + raw/native
 * 分离约定，折算走 Amount 接缝（后端 `transaction::amount`）。
 */

/** 物品生命周期状态：`in_use`（在用，摊到今天）/ `disposed`（已处置，摊到处置日）。 */
export type ItemStatus = 'in_use' | 'disposed'

/** 物品实体（读模型，全字段，对应后端 `models::item::Item`）。 */
export interface Item extends Syncable {
  id: string
  name: string
  /** 购买日期（YYYY-MM-DD）。 */
  purchase_date: string
  /** 总成本（原始币种，整数分）。 */
  total_cost_cents: number
  currency_code: string
  /** 总成本折算本位币（默认币种，整数分）。 */
  cost_native_cents: number
  status: ItemStatus
  /** 处置日期（仅 disposed；YYYY-MM-DD）。 */
  disposal_date: string | null
  /** 残值（仅 disposed 可填，整数分）。 */
  residual_value_cents: number | null
  note: string | null
  created_at: string
}

/** 创建物品入参（对应后端 `models::item::ItemInput`）。 */
export interface ItemInput {
  name: string
  /** 购买日期（YYYY-MM-DD）。 */
  purchase_date: string
  /** 总成本（原始币种，整数分），必须 > 0。 */
  total_cost_cents: number
  currency_code: string
  note?: string | null
}

/** 物品列表项：物品实体 + 每天使用成本快照（后端 `item::cost` 接缝计算）。 */
export interface ItemWithDailyCost extends Item {
  /** 已用天数：购买日 → 目标日的日历天数，含起止两端（在用 = 今天，已处置 = 处置日）。 */
  used_days: number
  /** 每天成本（分/天，小数，查询时刻快照）。 */
  per_day_cents: number
}

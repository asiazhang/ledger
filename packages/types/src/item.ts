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
  /** 关联购买交易 id（溯源，可空）；无「交易→物品」反向引用。 */
  purchase_transaction_id: string | null
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
  /** 可选关联的购买交易（expense）id：创建时提供（或修改时提供与既有不同的 id）
   * → 后端自动带出日期与基础成本；修改时提供相同 id 或省略 → 维持现状不重新带出。 */
  purchase_transaction_id?: string | null
}

/** 处置物品入参（issue #120，对应后端 `models::item::ItemDisposeInput`）。 */
export interface ItemDisposeInput {
  /** 处置日期（YYYY-MM-DD），必填；不得早于购买日期。 */
  disposal_date: string
  /** 残值（整数分，可选）：省略/null 视同无残值（分子 = 总成本）。 */
  residual_value_cents?: number | null
}

/** 物品列表项：物品实体 + 每天使用成本快照（后端 `item::cost` 接缝计算）。 */
export interface ItemWithDailyCost extends Item {
  /** 已用天数：购买日 → 目标日的日历天数，含起止两端（在用 = 今天，已处置 = 处置日）。 */
  used_days: number
  /** 成本分解分子（分）：总成本 − 残值，下限 0（在用未填残值时即总成本）。 */
  numerator_cents: number
  /** 每天成本（分/天，小数，查询时刻快照）。 */
  per_day_cents: number
}

/** 每天使用成本计算结果（issue #121，对应后端 `models::item::ItemDailyCost`）：
 * 与 `ItemWithDailyCost` 尾部三元组同一口径（`item::cost` 接缝），详情视图重算展示共用。 */
export interface ItemDailyCost {
  /** 已用天数：购买日 → 目标日（参考日或缺省目标日）的日历天数，含起止两端。 */
  used_days: number
  /** 成本分解分子（分）：总成本 − 残值，下限 0。 */
  numerator_cents: number
  /** 每天成本（分/天，小数），仅供展示。 */
  per_day_cents: number
}

/** 全部在用物品「每天成本合计」聚合结果（issue #122，对应后端
 * `models::item::ItemDailyTotal`）：多币种折算与合计全部在后端完成（同
 * `dashboard_overview` 的约定，前端不出现第二份口径表达式），dashboard 汇总卡消费。 */
export interface ItemDailyTotal {
  /** 默认币种代码（合计折算目标）。 */
  native_currency: string
  /** 每天成本合计（本位币分/天，小数）：Σ 各在用物品分子（折本位币）÷ 各自天数。 */
  per_day_cents: number
  /** 计入合计的在用物品件数（含分子为 0 的物品；已处置/已删除不计入）。 */
  item_count: number
}

import type { Syncable } from './common'

/**
 * 保单（Policy）领域类型（issue #360 / ADR-0051）：消费型保险合同的静态档案，
 * 与物品域 Item 同为独立领域概念（CONTEXT-insurance.md `Policy` 条目）。
 * 保司引用保险域自有独立字典（Insurer，issue #713 / ADR-0082，不复用商户）；
 * 保障期间止日可空（= 长期/终身）；
 * 保额可选、纯展示、不进任何金额口径（不折算、不聚合）。
 */

/** 保单实体（读模型，全字段，对应后端 `models::policy::Policy`）。 */
export interface Policy extends Syncable {
  id: string
  /** 保险公司（保司字典引用，ADR-0082）。 */
  insurer_id: string
  /** 保单号。 */
  policy_number: string
  /** 险种名称。 */
  product_name: string
  /** 保障期间起（YYYY-MM-DD）。 */
  start_date: string
  /** 保障期间止（YYYY-MM-DD）；null = 长期/终身。 */
  end_date: string | null
  /** 保额（整数分，可选）；纯展示，不进任何金额口径。 */
  coverage_amount_cents: number | null
  /** 保额币种（与保额成对：保额存在时必填）。 */
  coverage_currency_code: string | null
  note: string | null
  is_deleted: boolean
  created_at: string
}

/** 创建/编辑保单入参（对应后端 `models::policy::PolicyInput`，全量替换）。 */
export interface PolicyInput {
  insurer_id: string
  policy_number: string
  product_name: string
  /** 保障期间起（YYYY-MM-DD）。 */
  start_date: string
  /** 保障期间止（YYYY-MM-DD，可空 = 长期/终身）。 */
  end_date?: string | null
  /** 保额（整数分，可选）。 */
  coverage_amount_cents?: number | null
  /** 保额币种（保额存在时必填）。 */
  coverage_currency_code?: string | null
  note?: string | null
}

/** 逐保单视角统计行（对应后端 `models::policy::PolicyStats`，issue #363 / ADR-0051 决策 5/6）。 */
export interface PolicyStats {
  policy_id: string
  /** 折算基准币种（全局默认币种）：下列两个合计均为本位币口径。 */
  native_currency: string
  /** 累计已缴保费（本位币，分）：挂单保费流水忠实合计，不摊销、不落库。 */
  total_paid_native_cents: number
  /** 累计现金流入（本位币，分）：挂单现金流入流水忠实合计。 */
  total_inflow_native_cents: number
  /** 下期扣款日（YYYY-MM-DD）；null = 无活跃协议/无 pending 期次（界面不显示）。 */
  next_charge_date: string | null
  /** 到期态（实时推导，不持久化）：止日非空且早于今天 → 已到期；止日空 = 长期/终身。 */
  is_expired: boolean
}

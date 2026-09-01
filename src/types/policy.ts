import type { Syncable } from './common'

/**
 * 保单（Policy）领域类型（issue #360 / ADR-0051）：消费型保险合同的静态档案，
 * 与物品域 Item 同为独立领域概念（CONTEXT-insurance.md `Policy` 条目）。
 * 保司复用商户字典（Merchant）；保障期间止日可空（= 长期/终身）；
 * 保额可选、纯展示、不进任何金额口径（不折算、不聚合）。
 */

/** 保单实体（读模型，全字段，对应后端 `models::policy::Policy`）。 */
export interface Policy extends Syncable {
  id: string
  /** 保险公司（商户字典引用）。 */
  merchant_id: string
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
  merchant_id: string
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

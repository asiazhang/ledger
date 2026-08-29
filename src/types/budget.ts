import type { Syncable } from './common'

export type BudgetPeriod = 'monthly' | 'yearly'

export const BUDGET_PERIOD_LABELS: Record<BudgetPeriod, string> = {
  monthly: '按月',
  yearly: '按年',
}

export interface Budget extends Syncable {
  id: string
  category_id: string
  period: BudgetPeriod
  amount_cents: number
  /** 冻结残留（ADR：永久滚动预算）：仅创建时记录用途，不参与进度计算 */
  start_date: string
  created_at: string
}

export interface BudgetInput {
  category_id: string
  period?: BudgetPeriod
  amount_cents: number
  /** 仅记录用途（表单不再提供日期选择器，前端传创建当日） */
  start_date: string
}

/** 预算编辑入参（issue #184）：仅允许修改金额，分类/周期不可改（改法为删旧建新） */
export interface BudgetUpdateInput {
  amount_cents: number
}

export interface BudgetProgress {
  budget: Budget
  category_name: string
  spent_cents: number
  over_budget: boolean
}

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
  start_date: string
  created_at: string
}

export interface BudgetInput {
  category_id: string
  period?: BudgetPeriod
  amount_cents: number
  start_date: string
}

export interface BudgetProgress {
  budget: Budget
  category_name: string
  spent_cents: number
  over_budget: boolean
}

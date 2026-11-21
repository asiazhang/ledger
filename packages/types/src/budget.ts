import type { Syncable } from "./common";

export type BudgetPeriod = "monthly" | "yearly";

export const BUDGET_PERIODS: BudgetPeriod[] = ["monthly", "yearly"];

export interface Budget extends Syncable {
  id: string;
  category_id: string;
  period: BudgetPeriod;
  amount_cents: number;
  start_date: string;
  created_at: string;
}

export interface BudgetInput {
  category_id: string;
  period?: BudgetPeriod;
  amount_cents: number;
  start_date: string;
}

export interface BudgetUpdateInput {
  amount_cents: number;
  period?: BudgetPeriod;
}

export interface BudgetProgress {
  budget: Budget;
  category_name: string;
  spent_cents: number;
  over_budget: boolean;
}

export interface BudgetAnnualSummary {
  annual_budget_total_cents: number;
  spent_cents: number;
  over_budget: boolean;
}

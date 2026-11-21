import type { Syncable } from "./common";

export type SavingsGoalStatus = "active" | "archived";

export interface SavingsGoal extends Syncable {
  id: string;
  name: string;
  target_amount_cents: number;
  deadline: string | null;
  status: SavingsGoalStatus;
  planned_monthly_cents: number | null;
  account_id: string;
  created_at: string;
}

export interface SavingsGoalInput {
  name: string;
  target_amount_cents: number;
  deadline?: string | null;
}

export interface SavingsGoalUpdateInput {
  name: string;
  target_amount_cents: number;
  deadline: string | null;
  planned_monthly_cents: number | null;
}
export type SavingsGoalPaceSource = "plan" | "manual";

export interface SavingsGoalProgress {
  goal: SavingsGoal;
  saved_cents: number;
  remaining_cents: number;
  achieved: boolean;
  currency_code: string;
  pace_monthly_cents: number | null;
  pace_source: SavingsGoalPaceSource | null;
  eta_months: number | null;
  eta_month: string | null;
  required_monthly_cents: number | null;
  pace_delta_cents: number | null;
}

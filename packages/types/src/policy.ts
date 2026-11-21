import type { Syncable } from "./common";

export interface Policy extends Syncable {
  id: string;
  insurer_id: string;
  policy_number: string;
  product_name: string;
  start_date: string;
  end_date: string | null;
  coverage_amount_cents: number | null;
  coverage_currency_code: string | null;
  note: string | null;
  is_deleted: boolean;
  created_at: string;
}

export interface PolicyInput {
  insurer_id: string;
  policy_number: string;
  product_name: string;
  start_date: string;
  end_date?: string | null;
  coverage_amount_cents?: number | null;
  coverage_currency_code?: string | null;
  note?: string | null;
}

export interface PolicyStats {
  policy_id: string;
  native_currency: string;
  total_paid_native_cents: number;
  total_inflow_native_cents: number;
  next_charge_date: string | null;
  is_expired: boolean;
}

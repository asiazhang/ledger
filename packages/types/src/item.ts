import type { Syncable } from "./common";

export type ItemStatus = "in_use" | "disposed";

export interface Item extends Syncable {
  id: string;
  name: string;
  purchase_date: string;
  total_cost_cents: number;
  currency_code: string;
  cost_native_cents: number;
  status: ItemStatus;
  disposal_date: string | null;
  residual_value_cents: number | null;
  purchase_transaction_id: string | null;
  note: string | null;
  created_at: string;
}

export interface ItemInput {
  name: string;
  purchase_date: string;
  total_cost_cents: number;
  currency_code: string;
  note?: string | null;
  purchase_transaction_id?: string | null;
}

export interface ItemDisposeInput {
  disposal_date: string;
  residual_value_cents?: number | null;
}

export interface ItemWithDailyCost extends Item {
  used_days: number;
  numerator_cents: number;
  per_day_cents: number;
}

export interface ItemDailyCost {
  used_days: number;
  numerator_cents: number;
  per_day_cents: number;
}

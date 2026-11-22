import type { Syncable } from "./common";

export type PhysicalAssetStatus = "holding" | "disposed";

export interface PhysicalAsset extends Syncable {
  id: string;
  name: string;
  purchase_date: string | null;
  purchase_price_cents: number | null;
  purchase_currency_code: string | null;
  status: PhysicalAssetStatus;
  disposal_date: string | null;
  disposal_price_cents: number | null;
  disposal_currency_code: string | null;
  created_at: string;
  current_valuation_cents: number;
  current_valuation_currency_code: string;
  current_valuation_date: string;
  current_valuation_native_cents: number | null;
  native_currency: string;
}

export interface PhysicalAssetInput {
  name: string;
  purchase_date?: string | null;
  purchase_price_cents?: number | null;
  purchase_currency_code?: string | null;
  initial_valuation_cents?: number | null;
  initial_valuation_currency_code?: string | null;
  initial_valuation_date?: string | null;
}

export interface PhysicalAssetUpdateInput {
  name: string;
  purchase_date?: string | null;
  purchase_price_cents?: number | null;
  purchase_currency_code?: string | null;
}

export interface PhysicalAssetValuationInput {
  amount_cents?: number | null;
  currency_code?: string | null;
  valuation_date?: string | null;
}

export interface PhysicalAssetDisposeInput {
  disposal_date?: string | null;
  disposal_price_cents?: number | null;
  disposal_currency_code?: string | null;
}

export interface PhysicalAssetList {
  assets: PhysicalAsset[];
  holding_total_native_cents: number;
  native_currency: string;
}

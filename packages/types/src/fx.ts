export interface ExchangeRate {
  id: string;
  base_code: string;
  quote_code: string;
  rate: number;
  priced_at: string;
  source: string | null;
  updated_at: string;
  version: number;
  device_id: string;
}

export interface ExchangeRateInput {
  base_code: string;
  quote_code: string;
  rate: number;
  priced_at: string;
  source?: string | null;
}

export interface ExchangeRateSyncPersistReport {
  pairs: number;
  points: number;
  earliest: string | null;
  latest: string | null;
  manual_protected: number;
}

export interface ExchangeRateSyncReport {
  full_backfilled: boolean;
  persist: ExchangeRateSyncPersistReport;
}

export type FxSyncStage = "fetching" | "persisting";

export interface FxSyncProgress {
  stage: FxSyncStage;
  days_parsed: number | null;
}

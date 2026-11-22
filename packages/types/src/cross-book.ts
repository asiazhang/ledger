export type CrossBookBookStatus =
  | "included"
  | "locked"
  | "not_initialized"
  | "schema_mismatch"
  | "unreadable";

export interface CrossBookBookRow {
  id: string;
  name: string;
  status: CrossBookBookStatus;
}

export interface CrossBookInvestmentSummary {
  target_currency: string;
  converted: boolean;
  market_value_cents: number;
  unrealized_pnl_cents: number;
  cumulative_pnl_cents: number;
  investable_assets_cents: number;
  books: CrossBookBookRow[];
}

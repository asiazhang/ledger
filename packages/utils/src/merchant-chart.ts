import type { MerchantShare } from "@ledger/types";
import { paletteColor, sharePercent } from "./category-chart";

export interface MerchantTableRow {
  merchant_id: string;
  name: string;
  amount_cents: number;
  barPct: number;
  sharePct: number;
  transactionCount: number;
  color: string;
}

export function merchantTableRows(rows: MerchantShare[], totalCents: number): MerchantTableRow[] {
  const maxPositive = rows.reduce((m, r) => Math.max(m, r.amount_cents), 0);
  return rows.map((r, i) => ({
    merchant_id: r.merchant_id,
    name: r.merchant_name,
    amount_cents: r.amount_cents,
    barPct:
      maxPositive > 0 ? Math.round((Math.max(r.amount_cents, 0) / maxPositive) * 10000) / 100 : 0,
    sharePct: sharePercent(r.amount_cents, totalCents),
    transactionCount: r.transaction_count,
    color: paletteColor(i),
  }));
}

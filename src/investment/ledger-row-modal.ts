import type { InvestmentTransactionRow, TransactionModalRow } from "@ledger/types";
import { instrumentDisplayLabel } from "@/investment/instrument-display-label";

export function ledgerRowToModalRow(row: InvestmentTransactionRow): TransactionModalRow {
  return {
    id: row.id,
    kind: row.kind,
    date: row.date,
    note: row.note,
    account_id: row.account_id,
    to_account_id: null,
    funding_account_id: row.funding_account_id,
    category_id: null,
    merchant_id: null,
    policy_id: null,
    refund_of_transaction_id: null,
    amount_cents: row.amount_cents,
    currency_code: row.currency_code,
    amount_native_cents: row.amount_native_cents,
    source: {
      kind: "instrument",
      entity_id: row.instrument_id,
      display_name: instrumentDisplayLabel(row.symbol, row.instrument_name),
      status: null,
    },
    fundings: [],
    purchases: [],
    convert: null,
    source_order_no: row.source_order_no,
  };
}

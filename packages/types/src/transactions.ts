import type { Syncable } from "./common";

export type TransactionKind =
  | "income"
  | "expense"
  | "transfer"
  | "refund"
  | "buy"
  | "sell"
  | "convert"
  | "split"
  | "dividend";

export type TransactionSourceKind =
  | "installmentPlan"
  | "subscription"
  | "scheduledTransfer"
  | "policy"
  | "item"
  | "instrument";

export type TransactionSourceStatus = "cancelled" | "disposed" | "deleted";

export interface ConvertFields {
  to_instrument_id: string;
  to_symbol: string;
  to_quantity: number;
  out_amount_cents: number;
  in_amount_cents: number;
}

export interface TransactionSource {
  kind: TransactionSourceKind;
  entity_id: string;
  display_name: string;
  status: TransactionSourceStatus | null;
}

export type FxRateSource = "series" | "explicit";

export interface TransactionFunding {
  account_id: string;
  amount_cents: number;
  label: string | null;
  derived: boolean;
}

export interface TransactionPurchase {
  name: string;
  quantity: number;
  category_id: string | null;
  unit_price_cents: number | null;
}

export interface Transaction extends Syncable {
  id: string;
  kind: TransactionKind;
  amount_cents: number;
  currency_code: string;
  amount_native_cents: number;
  fx_rate_used: number | null;
  fx_rate_source: FxRateSource | null;
  account_id: string | null;
  to_account_id: string | null;
  funding_account_id: string | null;
  category_id: string | null;
  merchant_id: string | null;
  policy_id: string | null;
  source: TransactionSource | null;
  convert: ConvertFields | null;
  refund_of_transaction_id: string | null;
  fundings: TransactionFunding[];
  purchases: TransactionPurchase[];
  source_order_no: string | null;
  note: string | null;
  date: string;
  created_at: string;
}
export type TransactionModalRow = Pick<
  Transaction,
  | "id"
  | "kind"
  | "date"
  | "note"
  | "account_id"
  | "to_account_id"
  | "funding_account_id"
  | "category_id"
  | "merchant_id"
  | "policy_id"
  | "refund_of_transaction_id"
  | "amount_cents"
  | "currency_code"
  | "amount_native_cents"
  | "fundings"
  | "purchases"
  | "source"
  | "convert"
  | "source_order_no"
>;

export interface TransactionFundingInput {
  account_id: string;
  amount_cents: number;
  label?: string | null;
}

export interface TransactionPurchaseInput {
  name: string;
  quantity: number;
  category_id?: string | null;
  unit_price_cents?: number | null;
}

export interface TransactionInput {
  kind: TransactionKind;
  amount_cents: number;
  currency_code: string;
  account_id?: string | null;
  to_account_id?: string | null;
  funding_account_id?: string | null;
  funding?: TransactionFundingInput[] | null;
  category_id?: string | null;
  purchases?: TransactionPurchaseInput[] | null;
  merchant_id?: string | null;
  merchant_name?: string | null;
  policy_id?: string | null;
  refund_of_transaction_id?: string | null;
  source_order_no?: string | null;
  note?: string | null;
  date: string;
  instrument_id?: string | null;
  quantity?: number | null;
  price_cents?: number | null;
  fee_cents?: number | null;
  to_instrument_id?: string | null;
  to_quantity?: number | null;
  out_amount_cents?: number | null;
  in_amount_cents?: number | null;
  fx_rate?: number | null;
  idempotency_key?: string | null;
}

export type UpdateTransactionInput = Omit<TransactionInput, "idempotency_key">;

export interface TransactionListFilter {
  from?: string | null;
  to?: string | null;
  account_id?: string | null;
  involving_account_id?: string | null;
  merchant_id?: string | null;
  category_id?: string | null;
  uncategorized_only?: boolean | null;
  instrument_id?: string | null;
  kinds?: TransactionKind[] | null;
  limit?: number | null;
  page?: number;
  page_size?: number;
  hide_investment_related?: boolean | null;
}

export interface TransactionListResult {
  items: Transaction[];
  total: number;
}

export interface OrderAccountContribution {
  account_id: string;
  amount_cents: number;
}

export interface TransactionOrderSummary {
  source_order_no: string;
  row_count: number;
  total_amount_cents: number;
  currency_code: string | null;
  accounts: OrderAccountContribution[];
  items: Transaction[];
}

export interface TransactionSearchResult {
  items: Transaction[];
  total: number;
}

export interface TransactionSearchFilter {
  amountMinCents?: number | null;
  amountMaxCents?: number | null;
  dateFrom?: string | null;
  dateTo?: string | null;
}

export type CreateTransactionKind = Exclude<
  TransactionKind,
  "refund" | "convert" | "split" | "dividend"
>;

const TRANSACTION_KIND_PRESENCE = {
  income: true,
  expense: true,
  transfer: true,
  refund: true,
  buy: true,
  sell: true,
  convert: true,
  split: true,
  dividend: true,
} satisfies Record<TransactionKind, boolean>;

export const TRANSACTION_KINDS = Object.keys(TRANSACTION_KIND_PRESENCE) as TransactionKind[];

export type TransactionKindActivation = "edit" | "detail" | "none";

const TRANSACTION_KIND_ACTIVATION = {
  income: "edit",
  expense: "edit",
  transfer: "edit",
  refund: "none",
  buy: "edit",
  sell: "edit",
  convert: "detail",
  split: "detail",
  dividend: "detail",
} satisfies Record<TransactionKind, TransactionKindActivation>;

export function transactionKindActivation(kind: TransactionKind): TransactionKindActivation {
  return TRANSACTION_KIND_ACTIVATION[kind];
}

const CREATE_KIND_ENTRY = {
  expense: true,
  income: true,
  transfer: true,
  buy: false,
  sell: false,
} satisfies Record<CreateTransactionKind, boolean>;

export const CREATE_KINDS = (Object.keys(CREATE_KIND_ENTRY) as CreateTransactionKind[]).filter(
  (kind) => CREATE_KIND_ENTRY[kind],
);

export type TransactionPageCreateKind = {
  [K in keyof typeof CREATE_KIND_ENTRY]: (typeof CREATE_KIND_ENTRY)[K] extends true ? K : never;
}[keyof typeof CREATE_KIND_ENTRY];

export type CreateFormKind = CreateTransactionKind | "lend" | "borrow";

export const LENDING_CREATE_DIRECTIONS = ["lend", "borrow"] as const;

export interface CreateTransactionResult {
  success: boolean;
  duplicate: boolean;
  id: string | null;
  error: string | null;
}

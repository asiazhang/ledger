import type { TransactionInput, TransactionKind } from "@ledger/types";
import { toLocalDateISO } from "@ledger/utils/date";
import { yuanToCents, yuanToPrice } from "@ledger/money";
import { t } from "@ledger/i18n";

export interface ExpenseIncomeFormState {
  kind: "expense" | "income";
  amount: number | null;
  currencyCode: string;
  accountId: string | null;
  categoryId: string | null;
  merchantId: string | null;
  policyId: string | null;
  sourceOrderNo?: string | null;
  note: string;
  date: number;
}

export interface TransferFormState {
  amount: number | null;
  currencyCode: string;
  accountId: string | null;
  toAccountId: string | null;
  merchantId: string | null;
  note: string;
  date: number;
  sourceOrderNo?: string | null;
}

export interface RefundFormState {
  amount: number | null;
  currencyCode: string;
  accountId: string | null;
  refundOfTransactionId: string | null;
  note: string;
  date: number;
}

export interface TradeFormState {
  kind: "buy" | "sell";
  currencyCode: string;
  accountId: string | null;
  instrumentId: string | null;
  amount: number | null;
  quantity: number | null;
  price: number | null;
  fee: number | null;
  fundingAccountId: string | null;
  note: string;
  date: number;
  sourceOrderNo?: string | null;
}

interface KindMatrixRow {
  amount_cents?: 0;
  to_account_id: null;
  funding_account_id: null;
  category_id: null;
  merchant_id: null;
  policy_id: null;
  refund_of_transaction_id: null;
}

const KIND_FIELD_MATRIX: Record<TransactionKind, KindMatrixRow> = {
  income: {
    to_account_id: null,
    funding_account_id: null,
    category_id: null,
    merchant_id: null,
    policy_id: null,
    refund_of_transaction_id: null,
  },
  expense: {
    to_account_id: null,
    funding_account_id: null,
    category_id: null,
    merchant_id: null,
    policy_id: null,
    refund_of_transaction_id: null,
  },
  transfer: {
    to_account_id: null,
    funding_account_id: null,
    category_id: null,
    merchant_id: null,
    policy_id: null,
    refund_of_transaction_id: null,
  },
  refund: {
    to_account_id: null,
    funding_account_id: null,
    category_id: null,
    merchant_id: null,
    policy_id: null,
    refund_of_transaction_id: null,
  },
  buy: {
    amount_cents: 0,
    to_account_id: null,
    funding_account_id: null,
    category_id: null,
    merchant_id: null,
    policy_id: null,
    refund_of_transaction_id: null,
  },
  sell: {
    amount_cents: 0,
    to_account_id: null,
    funding_account_id: null,
    category_id: null,
    merchant_id: null,
    policy_id: null,
    refund_of_transaction_id: null,
  },
  convert: {
    amount_cents: 0,
    to_account_id: null,
    funding_account_id: null,
    category_id: null,
    merchant_id: null,
    policy_id: null,
    refund_of_transaction_id: null,
  },
  split: {
    amount_cents: 0,
    to_account_id: null,
    funding_account_id: null,
    category_id: null,
    merchant_id: null,
    policy_id: null,
    refund_of_transaction_id: null,
  },
  dividend: {
    to_account_id: null,
    funding_account_id: null,
    category_id: null,
    merchant_id: null,
    policy_id: null,
    refund_of_transaction_id: null,
  },
};

function fail(message: string): never {
  throw new Error(message);
}

function requireNonEmpty(value: string | null, label: string): string {
  if (value == null || !value.trim()) fail(t("transactions.validation.required", { label }));
  return value;
}

function requireAmountCents(amount: number | null, label: string): number {
  if (amount == null) fail(t("transactions.validation.required", { label }));
  const cents = yuanToCents(amount);
  if (cents == null) fail(t("transactions.validation.invalid", { label, value: amount }));
  return cents;
}

function requirePrice(price: number | null, label: string): number {
  if (price == null) fail(t("transactions.validation.required", { label }));
  const p = yuanToPrice(price);
  if (p == null) fail(t("transactions.validation.invalid", { label, value: price }));
  return p;
}

function requireDateISO(date: number): string {
  if (!Number.isFinite(date)) fail(t("transactions.validation.dateInvalid"));
  return toLocalDateISO(date);
}

function baseInput(
  kind: TransactionKind,
  fields: {
    amountCents?: number;
    currencyCode: string;
    accountId: string;
    note: string;
    date: string;
    sourceOrderNo?: string | null;
  },
): TransactionInput {
  const row = KIND_FIELD_MATRIX[kind];
  return {
    kind,
    amount_cents: fields.amountCents ?? row.amount_cents ?? fail(`${kind} 缺少金额`),
    to_account_id: row.to_account_id,
    funding_account_id: row.funding_account_id,
    category_id: row.category_id,
    merchant_id: row.merchant_id,
    policy_id: row.policy_id,
    refund_of_transaction_id: row.refund_of_transaction_id,
    currency_code: fields.currencyCode,
    account_id: fields.accountId,
    note: fields.note || null,
    source_order_no: fields.sourceOrderNo ?? null,
    date: fields.date,
  };
}

export function buildExpenseIncomeInput(state: ExpenseIncomeFormState): TransactionInput {
  return {
    ...baseInput(state.kind, {
      amountCents: requireAmountCents(state.amount, t("transactions.field.amount")),
      currencyCode: requireNonEmpty(state.currencyCode, t("transactions.field.currency")),
      accountId: requireNonEmpty(state.accountId, t("transactions.field.account")),
      note: state.note,
      sourceOrderNo: state.sourceOrderNo,
      date: requireDateISO(state.date),
    }),
    category_id: state.categoryId,
    merchant_id: state.merchantId,
    policy_id: state.policyId,
  };
}

export function buildTransferInput(state: TransferFormState): TransactionInput {
  return {
    ...baseInput("transfer", {
      amountCents: requireAmountCents(state.amount, t("transactions.field.amount")),
      currencyCode: requireNonEmpty(state.currencyCode, t("transactions.field.currency")),
      accountId: requireNonEmpty(state.accountId, t("transactions.field.fromAccount")),
      note: state.note,
      sourceOrderNo: state.sourceOrderNo,
      date: requireDateISO(state.date),
    }),
    to_account_id: requireNonEmpty(state.toAccountId, t("transactions.field.toAccount")),
    merchant_id: state.merchantId,
  };
}

export function buildRefundInput(state: RefundFormState): TransactionInput {
  return {
    ...baseInput("refund", {
      amountCents: requireAmountCents(state.amount, t("transactions.field.refundAmount")),
      currencyCode: requireNonEmpty(state.currencyCode, t("transactions.field.currency")),
      accountId: requireNonEmpty(state.accountId, t("transactions.field.account")),
      note: state.note,
      date: requireDateISO(state.date),
    }),
    refund_of_transaction_id: requireNonEmpty(
      state.refundOfTransactionId,
      t("transactions.field.originalExpense"),
    ),
  };
}

export function buildTradeInput(state: TradeFormState): TransactionInput {
  const fundAmountCents =
    state.amount == null ? null : requireAmountCents(state.amount, t("transactions.field.amount"));
  if (fundAmountCents != null && state.price != null) {
    fail(t("transactions.validation.fundAmountPriceConflict"));
  }
  const priceCents =
    fundAmountCents != null ? null : requirePrice(state.price, t("transactions.field.price"));
  return {
    ...baseInput(state.kind, {
      amountCents: fundAmountCents ?? undefined,
      currencyCode: requireNonEmpty(state.currencyCode, t("transactions.field.currency")),
      accountId: requireNonEmpty(state.accountId, t("transactions.field.investmentAccount")),
      note: state.note,
      sourceOrderNo: state.sourceOrderNo,
      date: requireDateISO(state.date),
    }),
    instrument_id: requireNonEmpty(state.instrumentId, t("transactions.field.instrument")),
    quantity: requireQuantity(state.quantity),
    funding_account_id: state.fundingAccountId,
    price_cents: priceCents,
    fee_cents:
      state.fee == null ? null : requireAmountCents(state.fee, t("transactions.field.fee")),
  };
}

function requireQuantity(quantity: number | null): number {
  if (quantity == null) fail(t("transactions.validation.quantityRequired"));
  if (!Number.isFinite(quantity))
    fail(t("transactions.validation.quantityInvalid", { value: quantity }));
  return quantity;
}

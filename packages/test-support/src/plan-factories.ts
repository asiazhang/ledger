import type {
  ScheduledTransaction,
  ScheduledTransactionOccurrence,
  ScheduledTransactionWithExt,
} from "@ledger/types";

export function makeSubscriptionPlan(
  partial: Partial<ScheduledTransaction> & { id: string },
  merchant_id: string | null = null,
): ScheduledTransactionWithExt {
  const core: ScheduledTransaction = {
    kind: "subscription",
    status: "active",
    account_id: "acc-1",
    category_id: "cat-1",
    amount_cents: 1500,
    currency_code: "CNY",
    recurrence_type: "monthly",
    recurrence_interval: 1,
    recurrence_day: null,
    start_date: "2026-01-01",
    note: "视频会员",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    ...partial,
  };
  return {
    core,
    merchant_id,
    policy_id: null,
    total_amount_cents: null,
    total_occurrences: null,
    to_account_id: null,
  };
}

export function makeInstallmentPlan(
  partial: Partial<ScheduledTransaction> & { id: string },
  total_amount_cents: number,
  total_occurrences: number,
  merchant_id: string | null = null,
): ScheduledTransactionWithExt {
  const core: ScheduledTransaction = {
    kind: "installment",
    status: "active",
    account_id: "acc-1",
    category_id: "cat-1",
    amount_cents: Math.floor(total_amount_cents / total_occurrences),
    currency_code: "CNY",
    recurrence_type: "monthly",
    recurrence_interval: 1,
    recurrence_day: null,
    start_date: "2026-01-01",
    note: "手机分期",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    ...partial,
  };
  return {
    core,
    merchant_id,
    policy_id: null,
    total_amount_cents,
    total_occurrences,
    to_account_id: null,
  };
}

export function makeTransferPlan(
  partial: Partial<ScheduledTransaction> & { id: string },
  to_account_id: string | null,
  total_occurrences: number | null = null,
): ScheduledTransactionWithExt {
  const core: ScheduledTransaction = {
    kind: "scheduled_transfer",
    status: "active",
    account_id: "acc-cny1",
    category_id: null,
    amount_cents: 50000,
    currency_code: "CNY",
    recurrence_type: "monthly",
    recurrence_interval: 1,
    recurrence_day: null,
    start_date: "2026-01-01",
    note: null,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    ...partial,
  };
  return {
    core,
    merchant_id: null,
    policy_id: null,
    total_amount_cents: null,
    total_occurrences,
    to_account_id,
  };
}

export function makeOccurrence(
  partial: Partial<ScheduledTransactionOccurrence> & { id: string },
): ScheduledTransactionOccurrence {
  return {
    scheduled_transaction_id: "plan-1",
    scheduled_date: "2026-03-01",
    status: "pending",
    transaction_id: null,
    amount_cents: 1500,
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    version: 1,
    device_id: "test",
    is_deleted: false,
    ...partial,
  };
}

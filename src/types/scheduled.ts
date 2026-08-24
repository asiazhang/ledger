export type ScheduledKind = 'installment' | 'subscription' | 'scheduled_transfer'
export type ScheduledStatus = 'active' | 'paused' | 'cancelled' | 'completed'
export type RecurrenceType = 'daily' | 'weekly' | 'monthly' | 'yearly'

export interface ScheduledTransaction {
  id: string
  kind: ScheduledKind
  status: ScheduledStatus
  account_id: string
  category_id: string | null
  amount_cents: number
  currency_code: string
  recurrence_type: RecurrenceType
  recurrence_interval: number
  recurrence_day: number | null
  start_date: string
  note: string | null
  created_at: string
  updated_at: string
  version: number
  device_id: string
  is_deleted: boolean
}

export interface ScheduledTransactionOccurrence {
  id: string
  scheduled_transaction_id: string
  scheduled_date: string
  status: 'pending' | 'processing' | 'completed' | 'failed' | 'cancelled'
  transaction_id: string | null
  amount_cents: number
  created_at: string
  updated_at: string
  version: number
  device_id: string
  is_deleted: boolean
}

export interface ScheduledTransactionWithExt {
  core: ScheduledTransaction
  counterparty: string | null
  total_amount_cents: number | null
  total_occurrences: number | null
  to_account_id: string | null
}

export interface ScheduledTransactionDetail {
  core: ScheduledTransaction
  extension: InstallmentPlan | SubscriptionPlan | ScheduledTransferPlan
  pending_occurrences: ScheduledTransactionOccurrence[]
  completed_occurrences: number
}

export interface InstallmentPlan {
  scheduled_transaction_id: string
  counterparty: string | null
  total_amount_cents: number
  total_occurrences: number
}

export interface SubscriptionPlan {
  scheduled_transaction_id: string
  counterparty: string | null
}

export interface ScheduledTransferPlan {
  scheduled_transaction_id: string
  to_account_id: string
  total_occurrences: number | null
}

export interface CreateScheduledInput {
  kind: ScheduledKind
  account_id: string
  category_id?: string | null
  amount_cents: number
  currency_code: string
  recurrence_type: RecurrenceType
  recurrence_interval: number
  recurrence_day?: number | null
  start_date: string
  note?: string | null
  counterparty?: string | null
  total_amount_cents?: number | null
  total_occurrences?: number | null
  to_account_id?: string | null
}

export interface UpdateStatusInput {
  id: string
  new_status: ScheduledStatus
}

export interface ExecuteOccurrenceInput {
  occurrence_id: string
}

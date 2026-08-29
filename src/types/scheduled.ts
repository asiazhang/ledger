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
  /** 商户 id（installment/subscription 可携带；scheduled_transfer 恒为 null） */
  merchant_id: string | null
  total_amount_cents: number | null
  total_occurrences: number | null
  to_account_id: string | null
}

export interface ScheduledTransactionDetail {
  core: ScheduledTransaction
  extension: InstallmentPlan | SubscriptionPlan | ScheduledTransferPlan
  pending_occurrences: ScheduledTransactionOccurrence[]
  completed_occurrences: number
  /** 失败期次（issue #205）：期次详情弹窗「重试」的数据源 */
  failed_occurrences: ScheduledTransactionOccurrence[]
  /** 已完成期次列表（issue #205）：期次详情弹窗展示每期执行状态；计数字段为既有契约保留 */
  completed_occurrence_list: ScheduledTransactionOccurrence[]
}

export interface InstallmentPlan {
  scheduled_transaction_id: string
  merchant_id: string | null
  total_amount_cents: number
  total_occurrences: number
}

export interface SubscriptionPlan {
  scheduled_transaction_id: string
  merchant_id: string | null
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
  /** 商户 id（installment/subscription 可携带；scheduled_transfer 后端拒绝携带） */
  merchant_id?: string | null
  total_amount_cents?: number | null
  total_occurrences?: number | null
  to_account_id?: string | null
}

export interface UpdateStatusInput {
  id: string
  new_status: ScheduledStatus
}

/**
 * 订阅编辑输入（issue #162，ADR-0023 决策三）：仅允许金额以外字段
 * （备注、分类、扣款账户、商户）。`amount_cents` / `total_amount_cents` 为兼容哨兵：
 * 请求一旦携带即被后端显式拒绝——改价 = 取消旧计划 + 新建。
 */
export interface UpdateSubscriptionInput {
  id: string
  account_id: string
  category_id?: string | null
  note?: string | null
  /** 商户 id（issue #190）：可改商户，编辑只影响未来期次 */
  merchant_id?: string | null
  amount_cents?: number
  total_amount_cents?: number
}

export interface ExecuteOccurrenceInput {
  occurrence_id: string
}

// ---------------------------------------------------------------------------
// 订阅花费——实际花费口径（issue #160，ADR-0023 决策二）
// ---------------------------------------------------------------------------

/** 逐订阅行：计划基础信息 + 该订阅本月/本年实际花费（本位币）。 */
export interface SubscriptionSpendRow {
  plan_id: string
  note: string | null
  /** 商户名（后端左联 merchants 现名）：改名即时生效，软删后历史计划照常显示 */
  merchant_name: string | null
  /** 计划状态；取消/暂停不影响其历史实际花费 */
  status: ScheduledStatus
  /** 每期金额（计划币种，原始口径） */
  amount_cents: number
  currency_code: string
  /** 该订阅本月实际花费（本位币，分） */
  this_month_native_cents: number
  /** 该订阅本年实际花费（本位币，分） */
  this_year_native_cents: number
}

/** 单个日历月的订阅实际花费（本位币，分）。 */
export interface SubscriptionMonthSpend {
  /** 日历月，`YYYY-MM` */
  month: string
  native_cents: number
}

/** `subscription_spend_overview` 命令返回的订阅花费总览（本位币口径，单位：分）。 */
export interface SubscriptionSpendOverview {
  /** 折算基准币种（全局默认币种） */
  native_currency: string
  /** 本月实际花费合计（分） */
  this_month_native_cents: number
  /** 本年实际花费合计（分） */
  this_year_native_cents: number
  /** 过去 12 个日历月逐月实际花费（含当月，旧→新，无扣款月补 0） */
  months: SubscriptionMonthSpend[]
  /** 逐订阅行（含已取消/暂停计划） */
  rows: SubscriptionSpendRow[]
  /** 折算月成本合计（分）：只统计 active 计划，系数收口在后端（issue #161，ADR-0023） */
  projected_month_native_cents: number
  /** 折算年成本合计（分）= 折算月成本 × 12；纯展示，不落库、不进流水与预算 */
  projected_year_native_cents: number
}

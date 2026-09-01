import type { TransactionInput, TransactionKind } from '@/types'
import { toLocalDateISO } from '@/utils/date'
import { yuanToCents, yuanToPrice } from '@/utils/money'
import { t } from '@/i18n'

/**
 * TransactionInput 装配器（issue #215）：「记一笔」表单状态 → 完整 TransactionInput
 * 的唯一前端装配接缝（词汇表见 docs/contexts/CONTEXT-core.md「TransactionInput 装配器」）。
 *
 * 同步纯函数，无 Vue/store/api 依赖；同一装配结果供创建（api.createTransaction）与
 * 编辑（api.updateTransaction）两路共用——TransactionInput 与 UpdateTransactionInput
 * 字段同构（仅差不可编辑的 idempotency_key，表单不传）。
 *
 * 边界：
 * - per-kind 字段矩阵（各 kind 填什么、留什么 null）收敛于 KIND_FIELD_MATRIX 一处，
 *   新增表单形态只改矩阵；
 * - 元转分全仓只有 yuanToCents 一处（单价走 yuanToPrice 万分之一元，ADR-0038）、
 *   日期只有 toLocalDateISO 一处，此处不另写算法；
 * - 商户解析（resolveMerchantId 五分支）与语义校验（金额 > 0、转出≠转入等）留表单层；
 * - 非法输入 fail fast：throw 中文错误，不静默兜底。
 *
 * 四个表单 composable（useCategoryForm / useTransferForm / useRefundForm /
 * useInvestmentForm）已接入本接缝（issue #216）：表单只做语义校验与提交路由，
 * wire 字段拼装不再散落调用方。
 */

/** 支出/收入表单形态（useCategoryForm 表单状态原样；merchantId 为已解析的商户 id） */
export interface ExpenseIncomeFormState {
  kind: 'expense' | 'income'
  /** 金额（元）；null 属非法输入 */
  amount: number | null
  currencyCode: string
  accountId: string | null
  categoryId: string | null
  merchantId: string | null
  note: string
  /** 本地日期时间戳（日期选择器值） */
  date: number
}

/** 转账表单形态（useTransferForm 表单状态原样；转出=转入属语义校验，留表单层） */
export interface TransferFormState {
  amount: number | null
  currencyCode: string
  accountId: string | null
  toAccountId: string | null
  note: string
  date: number
}

/** 退款表单形态（useRefundForm 表单状态原样；账户/币种由后端继承原支出，此处照表单原样装配） */
export interface RefundFormState {
  amount: number | null
  currencyCode: string
  accountId: string | null
  refundOfTransactionId: string | null
  note: string
  date: number
}

/** 买入/卖出表单形态（useInvestmentForm 表单状态原样）。录入权威按标的类型分流
 * （issue #302 / ADR-0038）：场外基金 = 金额 + 确认份额必填（amount 非空、price 恒 null），
 * 行金额以确认单整分金额为权威、单价由后端反算；其余 = 数量 + 单价（price 非空、
 * amount 恒 null），成交金额由后端行为层按数量×单价±费用重算。 */
export interface TradeFormState {
  kind: 'buy' | 'sell'
  currencyCode: string
  accountId: string | null
  instrumentId: string | null
  /** 确认单金额（元，基金申赎权威口径）；非基金形态恒 null */
  amount: number | null
  quantity: number | null
  /** 单价（元，非基金形态）；基金形态恒 null（单价反算，不落 wire） */
  price: number | null
  /** 手续费（元）；null 表示未填 → fee_cents: null（而非 0） */
  fee: number | null
  note: string
  date: number
}

/**
 * 矩阵行形状：每个 kind 一行，完整列出全部关联字段的处置（null = 不由表单承载）。
 * 行内必须穷尽四个关联字段（新增 kind 时逐字段显式决策，不允许缺省）。
 */
interface KindMatrixRow {
  /** buy/sell 金额占位恒 0（成交金额由后端按数量×单价±费用重算）；其余 kind 不设，金额由表单承载 */
  amount_cents?: 0
  to_account_id: null
  category_id: null
  merchant_id: null
  refund_of_transaction_id: null
}

/**
 * per-kind 字段矩阵：各 kind 填什么、留什么 null 收敛于此一处；表单承载的字段
 * 由各入口在装配结果上覆写。新增表单形态只改此矩阵。
 */
const KIND_FIELD_MATRIX: Record<TransactionKind, KindMatrixRow> = {
  income: { to_account_id: null, category_id: null, merchant_id: null, refund_of_transaction_id: null },
  expense: { to_account_id: null, category_id: null, merchant_id: null, refund_of_transaction_id: null },
  transfer: { to_account_id: null, category_id: null, merchant_id: null, refund_of_transaction_id: null },
  refund: { to_account_id: null, category_id: null, merchant_id: null, refund_of_transaction_id: null },
  buy: {
    amount_cents: 0,
    to_account_id: null,
    category_id: null,
    merchant_id: null,
    refund_of_transaction_id: null,
  },
  sell: {
    amount_cents: 0,
    to_account_id: null,
    category_id: null,
    merchant_id: null,
    refund_of_transaction_id: null,
  },
}

/** fail fast：非法表单状态抛中文错误，不静默兜底 */
function fail(message: string): never {
  throw new Error(message)
}

/** 必填短文本（id、币种代码等）：null 或空白视为非法 */
function requireNonEmpty(value: string | null, label: string): string {
  if (value == null || !value.trim()) fail(t('transactions.validation.required', { label }))
  return value
}

/** 元 → 分（yuanToCents 单点口径）：缺失或非法数值 fail fast */
function requireAmountCents(amount: number | null, label: string): number {
  if (amount == null) fail(t('transactions.validation.required', { label }))
  const cents = yuanToCents(amount)
  if (cents == null) fail(t('transactions.validation.invalid', { label, value: amount }))
  return cents
}

/** 元 → 万分之一元（yuanToPrice 单点口径，价格刻度见 ADR-0038）：缺失或非法数值 fail fast */
function requirePrice(price: number | null, label: string): number {
  if (price == null) fail(t('transactions.validation.required', { label }))
  const p = yuanToPrice(price)
  if (p == null) fail(t('transactions.validation.invalid', { label, value: price }))
  return p
}

/** 本地日期时间戳 → YYYY-MM-DD（toLocalDateISO 单点口径）：非法时间戳 fail fast */
function requireDateISO(date: number): string {
  if (!Number.isFinite(date)) fail(t('transactions.validation.dateInvalid'))
  return toLocalDateISO(date)
}

/** 各入口共享的公共字段装配；note 沿表单现状：空串 → null */
function baseInput(
  kind: TransactionKind,
  fields: {
    /** 表单承载的金额（已元转分）；矩阵已含 amount_cents 占位的 kind（buy/sell）不传 */
    amountCents?: number
    currencyCode: string
    accountId: string
    note: string
    date: string
  },
): TransactionInput {
  const row = KIND_FIELD_MATRIX[kind]
  return {
    kind,
    // 矩阵先行落位：关联字段占位（含 buy/sell 的 amount_cents: 0），表单承载字段由各入口覆写
    amount_cents: fields.amountCents ?? row.amount_cents ?? fail(`${kind} 缺少金额`),
    to_account_id: row.to_account_id,
    category_id: row.category_id,
    merchant_id: row.merchant_id,
    refund_of_transaction_id: row.refund_of_transaction_id,
    currency_code: fields.currencyCode,
    account_id: fields.accountId,
    note: fields.note || null,
    date: fields.date,
  }
}

/** 支出/收入表单状态 → TransactionInput（merchantId 须已经表单层 resolveMerchantId 解析） */
export function buildExpenseIncomeInput(state: ExpenseIncomeFormState): TransactionInput {
  return {
    ...baseInput(state.kind, {
      amountCents: requireAmountCents(state.amount, t('transactions.field.amount')),
      currencyCode: requireNonEmpty(state.currencyCode, t('transactions.field.currency')),
      accountId: requireNonEmpty(state.accountId, t('transactions.field.account')),
      note: state.note,
      date: requireDateISO(state.date),
    }),
    category_id: state.categoryId,
    merchant_id: state.merchantId,
  }
}

/** 转账表单状态 → TransactionInput */
export function buildTransferInput(state: TransferFormState): TransactionInput {
  return {
    ...baseInput('transfer', {
      amountCents: requireAmountCents(state.amount, t('transactions.field.amount')),
      currencyCode: requireNonEmpty(state.currencyCode, t('transactions.field.currency')),
      accountId: requireNonEmpty(state.accountId, t('transactions.field.fromAccount')),
      note: state.note,
      date: requireDateISO(state.date),
    }),
    to_account_id: requireNonEmpty(state.toAccountId, t('transactions.field.toAccount')),
  }
}

/** 退款表单状态 → TransactionInput */
export function buildRefundInput(state: RefundFormState): TransactionInput {
  return {
    ...baseInput('refund', {
      amountCents: requireAmountCents(state.amount, t('transactions.field.refundAmount')),
      currencyCode: requireNonEmpty(state.currencyCode, t('transactions.field.currency')),
      accountId: requireNonEmpty(state.accountId, t('transactions.field.account')),
      note: state.note,
      date: requireDateISO(state.date),
    }),
    refund_of_transaction_id: requireNonEmpty(state.refundOfTransactionId, t('transactions.field.originalExpense')),
  }
}

/** 买入/卖出表单状态 → TransactionInput。录入权威按形态分流：amount 非空 = 基金
 * 申赎（amount_cents 权威、price_cents null 由后端反算）；price 非空 = 其余类型
 * （amount_cents 恒 0 占位、单价落 wire）；两者互斥，同供属非法状态 fail fast。 */
export function buildTradeInput(state: TradeFormState): TransactionInput {
  const fundAmountCents =
    state.amount == null ? null : requireAmountCents(state.amount, t('transactions.field.amount'))
  if (fundAmountCents != null && state.price != null) {
    fail(t('transactions.validation.fundAmountPriceConflict'))
  }
  // 非基金形态：单价必填（缺失在此 fail fast，不静默落 null）；基金形态恒 null。
  const priceCents = fundAmountCents != null ? null : requirePrice(state.price, t('transactions.field.price'))
  return {
    ...baseInput(state.kind, {
      // 基金：矩阵占位 0 被权威金额覆写；其余：矩阵占位 0（后端重算行金额）
      amountCents: fundAmountCents ?? undefined,
      currencyCode: requireNonEmpty(state.currencyCode, t('transactions.field.currency')),
      accountId: requireNonEmpty(state.accountId, t('transactions.field.investmentAccount')),
      note: state.note,
      date: requireDateISO(state.date),
    }),
    instrument_id: requireNonEmpty(state.instrumentId, t('transactions.field.instrument')),
    quantity: requireQuantity(state.quantity),
    // 单价是价格列（万分之一元刻度，ADR-0038），与金额列（分）换算口径不同；
    // 基金形态不落单价（null → 后端按金额 ∓ 费用 ÷ 份额反算）
    price_cents: priceCents,
    fee_cents: state.fee == null ? null : requireAmountCents(state.fee, t('transactions.field.fee')),
  }
}

/** 交易数量：可为小数（股数/份额），仅要求有限数值 */
function requireQuantity(quantity: number | null): number {
  if (quantity == null) fail(t('transactions.validation.quantityRequired'))
  if (!Number.isFinite(quantity)) fail(t('transactions.validation.quantityInvalid', { value: quantity }))
  return quantity
}

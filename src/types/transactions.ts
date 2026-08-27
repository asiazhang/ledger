import type { Syncable } from './common'

export type TransactionKind = 'income' | 'expense' | 'transfer' | 'refund' | 'buy' | 'sell'

export interface Transaction extends Syncable {
  id: string
  kind: TransactionKind
  amount_cents: number
  currency_code: string
  amount_native_cents: number
  account_id: string
  to_account_id: string | null
  category_id: string | null
  refund_of_transaction_id: string | null
  note: string | null
  date: string
  created_at: string
}

export interface TransactionInput {
  kind: TransactionKind
  amount_cents: number
  currency_code: string
  account_id: string
  to_account_id?: string | null
  category_id?: string | null
  refund_of_transaction_id?: string | null
  note?: string | null
  date: string
  instrument_id?: string | null
  quantity?: number | null
  price_cents?: number | null
  fee_cents?: number | null
  /** 客户端提供的、内容无关的导入幂等键（指向"该交易来自源文件哪一行"） */
  idempotency_key?: string | null
}

/** 交易列表查询过滤条件（服务端分页 + 过滤） */
export interface TransactionListFilter {
  /** 起始日期（含），YYYY-MM-DD */
  from?: string | null
  /** 结束日期（含），YYYY-MM-DD */
  to?: string | null
  /** 按转出账户过滤 */
  account_id?: string | null
  /** 涉及账户过滤（account_id 或 to_account_id 命中即算，含转入的转账） */
  involving_account_id?: string | null
  /** income / expense / transfer / buy / sell / refund */
  kind?: TransactionKind | null
  /** 取前 N 条（仪表盘"最近 N 条"场景），与分页互斥：传 page_size 时分页路径生效 */
  limit?: number | null
  /** 页码，从 1 开始，默认 1 */
  page?: number
  /** 每页条数，缺省返回全部（total 恒返回） */
  page_size?: number
}

/** 交易列表分页结果 */
export interface TransactionListResult {
  items: Transaction[]
  /** 满足过滤条件的未删除交易总数 */
  total: number
}

/** 交易搜索分页结果（服务端分页） */
export interface TransactionSearchResult {
  items: Transaction[]
  /** 命中总数（供「命中 N 条」与分页） */
  total: number
  /** 索引是否可能滞后：存在尚未刷新的写入时 true（后台定时刷新，周期内不立即可搜） */
  stale: boolean
}

/** 交易搜索筛选条件（与关键字 AND 组合；全部可选、单边可用） */
export interface TransactionSearchFilter {
  /** 金额下限（整数分） */
  amountMinCents?: number | null
  /** 金额上限（整数分） */
  amountMaxCents?: number | null
  /** 起始日期（含），YYYY-MM-DD */
  dateFrom?: string | null
  /** 结束日期（含），YYYY-MM-DD */
  dateTo?: string | null
}

export type CreateTransactionKind = Exclude<TransactionKind, 'refund'>

export const TRANSACTION_KIND_LABELS: Record<TransactionKind, string> = {
  income: '收入',
  expense: '支出',
  transfer: '转账',
  refund: '退款',
  buy: '买入',
  sell: '卖出',
}

/** 「记一笔」分裂按钮可创建的类型：枚举对象以 Record<CreateTransactionKind, true> 表达，
 * 新增 kind 而未更新此表时编译报错（穷尽性由类型系统保证，下拉不会静默漏项）。 */
const CREATE_KIND_MAP = {
  expense: true,
  income: true,
  transfer: true,
  buy: true,
  sell: true,
} satisfies Record<CreateTransactionKind, true>

/** 「记一笔」入口可选类型（不含 refund：退款已移出表单域，入口由交易条目
 * 右键菜单承接，独立 ticket 落地前处于过渡态）。 */
export const CREATE_KINDS = Object.keys(CREATE_KIND_MAP) as CreateTransactionKind[]

export interface CreateTransactionResult {
  success: boolean
  duplicate: boolean
  id: string | null
  error: string | null
}

import type { Syncable } from './common'

export type TransactionKind = 'income' | 'expense' | 'transfer' | 'refund' | 'buy' | 'sell' | 'convert' | 'split'

/** 交易来源类型闭集（spec #704 / issue #706，词汇表「来源列」）：定时计划三形态 /
 * 保单 / 物品 / 标的；wire 字面与后端枚举（camelCase）同源，与 source-jump.ts
 * 深模块词表一致（点击 → 跳转目标计算的入参闭集）。 */
export type TransactionSourceKind =
  | 'installmentPlan'
  | 'subscription'
  | 'scheduledTransfer'
  | 'policy'
  | 'item'
  | 'instrument'

/** 来源状态闭集（spec #704，可空字段）：已取消计划 / 已处置物品 / 软删保单。 */
export type TransactionSourceStatus = 'cancelled' | 'disposed' | 'deleted'

/** 基金转换两腿扩展（ADR-0099）：一笔 convert 两腿的标的、份额与两侧确认金额。
 * 行金额锚点（`amount_native_cents`）是结转成本而非确认单金额，展示口径读本扩展。 */
export interface ConvertFields {
  /** 转入标的（转出标的恒为行的 instrument_id） */
  to_instrument_id: string
  /** 转入标的代码（JOIN instruments 带出的展示字段，移动卡片「A → B」消费） */
  to_symbol: string
  /** 转入份额 */
  to_quantity: number
  /** 转出金额（分，确认单权威） */
  out_amount_cents: number
  /** 转入金额（分，确认单权威） */
  in_amount_cents: number
}

/** 交易行来源（读时反查推导，零迁移）：仅列表/搜索命令填充，其余返回点为 null。 */
export interface TransactionSource {
  kind: TransactionSourceKind
  /** 来源实体 id */
  entity_id: string
  /** 展示名（保单 = 险种名；其余口径见各消费票） */
  display_name: string
  /** 来源状态（可空）：软删保单 = deleted */
  status: TransactionSourceStatus | null
}

export interface Transaction extends Syncable {
  id: string
  kind: TransactionKind
  amount_cents: number
  currency_code: string
  amount_native_cents: number
  account_id: string
  to_account_id: string | null
  /** 可选出资账户（issue #935 / ADR-0096）：仅 buy/sell 可携带，读投影恒返回（无则 null） */
  funding_account_id: string | null
  category_id: string | null
  merchant_id: string | null
  /** 可选保单引用（issue #361 / ADR-0051）：仅 expense/income 可挂，后端行为层准入 */
  policy_id: string | null
  /** 来源列（spec #704 / issue #706）：仅列表/搜索读路径填充，其余返回点为 null */
  source: TransactionSource | null
  /** 转换两腿扩展（ADR-0099）：仅 kind = convert 的行由列表/搜索读路径填充（其余 null）；
   * 行金额锚点是结转成本，列表金额列展示转出金额须读本扩展 */
  convert: ConvertFields | null
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
  /** 可选出资账户（issue #936 / ADR-0096）：仅 buy/sell 可携带（后端行为层准入），
   * 缺省即「结算账户 = 投资账户」；编辑路径全字段替换须显式携带，避免静默抹字段 */
  funding_account_id?: string | null
  category_id?: string | null
  /** 商户引用（expense/refund/income 可携带；transfer/buy/sell/dividend/split/convert 后端行为层拒绝） */
  merchant_id?: string | null
  /** 商户名字符串（AI 导入契约，issue #194）：后端精确匹配在用商户名，命中复用、未命中即建；
   * 与 merchant_id 互斥，与商户名归一化责任在后端 */
  merchant_name?: string | null
  /** 可选保单引用（issue #361 / ADR-0051 决策 3）：仅 expense/income 可携带，其余 kind 后端行为层拒绝；
   * 引用不存在的保单返回错误（中文，可读回自纠） */
  policy_id?: string | null
  refund_of_transaction_id?: string | null
  note?: string | null
  date: string
  instrument_id?: string | null
  quantity?: number | null
  price_cents?: number | null
  fee_cents?: number | null
  /** 转入标的 id（仅 convert 需提供，ADR-0099）：与 instrument_id（转出标的）必须不同 */
  to_instrument_id?: string | null
  /** 转入份额（仅 convert 需提供，必须 > 0） */
  to_quantity?: number | null
  /** 转出金额（仅 convert 需提供，整数分，必须 > 0）：确认单转出端金额（列表展示口径） */
  out_amount_cents?: number | null
  /** 转入金额（仅 convert 需提供，整数分，必须 > 0）：确认单转入端金额 */
  in_amount_cents?: number | null
  /** 客户端提供的、内容无关的导入幂等键（指向"该交易来自源文件哪一行"） */
  idempotency_key?: string | null
}

/** 交易修改入参（`PUT /api/v1/transactions/{id}` 与 IPC `update_transaction` 共用，issue #178）。
 * 与 TransactionInput 的唯一差异是不含 idempotency_key：幂等键不可编辑（只在导入时落定，
 * 编辑不改变导入身份），提交时同一对象形状分派创建/更新两路。 */
export type UpdateTransactionInput = Omit<TransactionInput, 'idempotency_key'>

/** 交易列表查询过滤条件（服务端分页 + 过滤） */
export interface TransactionListFilter {
  /** 起始日期（含），YYYY-MM-DD */
  from?: string | null
  /** 结束日期（含），YYYY-MM-DD */
  to?: string | null
  /** 按转出账户过滤 */
  account_id?: string | null
  /** 涉及账户过滤（account_id、to_account_id 或出资账户 funding_account_id 任一命中即算） */
  involving_account_id?: string | null
  /** 按商户过滤（issue #191）：命中该商户全部未删除交易，软删商户同样可过滤 */
  merchant_id?: string | null
  /** 按分类精确过滤（issue #377）：精确匹配不含子分类，软删分类的历史交易同样可过滤 */
  category_id?: string | null
  /** 仅无分类（issue #377）：true 时仅返回无分类交易；与 category_id 同携按 AND 组合 */
  uncategorized_only?: boolean | null
  /** 按标的过滤（ADR-0107）：命中证券交易扩展表中该标的的 buy/sell 行，
   * convert 任一腿命中即算（转入腿同算）；与其余维度 AND 组合 */
  instrument_id?: string | null
  /** 类型集合过滤（spec #1025 起为唯一类型维度，手动多选 + 下钻载荷共用）：命中集合内
   * 各类型（维度内取或，与其余维度 AND 组合）；空集合视为未携带（不过滤）。
   * 原单值 kind 查询参数已移除（BREAKING），单值亦经本参数传递 */
  kinds?: TransactionKind[] | null
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
}

/** 备注拼音回填失败阶段（issue #513，阶段文案经文案资源本地化） */
export type NotePinyinRepairStage = 'probe' | 'read' | 'begin' | 'write' | 'commit'

/** 备注拼音回填失败原因：失败阶段 + 底层错误消息（诊断用） */
export interface NotePinyinRepairFailure {
  stage: NotePinyinRepairStage
  message: string
}

/** 备注拼音一键修复报告（issue #513）：回填行数 / 是否收敛 / 失败原因 */
export interface NotePinyinRepairReport {
  /** 本次实际补齐的 NULL 积压行数（幂等：重复执行为 0） */
  backfilled: number
  /** 结束后积压是否清零（无备注行的 NULL 列不构成积压） */
  converged: boolean
  /** 失败原因（null = 全程无失败） */
  failure: NotePinyinRepairFailure | null
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

/** 「记一笔」可创建的类型：不含 refund（退款入口由交易条目右键菜单承接）；
 * 基金转换（convert）是无现金腿 kind，界面无手工录入入口（ADR-0106 决策 10 / #1048），
 * 写入面由 AI 导入 / HTTP 契约承担——移除后穷尽表仍由类型系统守门。 */
export type CreateTransactionKind = Exclude<TransactionKind, 'refund' | 'convert' | 'split'>

/** 前端交易类型闭集（穷尽表驱动）；显示标签在文案资源 transactions.kind.*（i18n，ADR-0049） */
const TRANSACTION_KIND_PRESENCE = {
  income: true,
  expense: true,
  transfer: true,
  refund: true,
  buy: true,
  sell: true,
  convert: true,
  split: true,
} satisfies Record<TransactionKind, boolean>

export const TRANSACTION_KINDS = Object.keys(TRANSACTION_KIND_PRESENCE) as TransactionKind[]

/** 交易 kind 的行激活形态闭集（穷尽表驱动，ADR-0106 决策 10 / #1048）：
 * - `edit`：行激活进可编辑表单（行菜单「编辑」）；
 * - `detail`：只读详情——「无现金腿」kind 无创建 / 编辑 / 软删写入口，只保留只读呈现；
 * - `none`：无行激活（refund 破坏关联语义，仅留行菜单软删）。
 * 新增 kind 而未更新此表时编译报错；split 落地（#1045）按件补行。 */
export type TransactionKindActivation = 'edit' | 'detail' | 'none'

const TRANSACTION_KIND_ACTIVATION = {
  income: 'edit',
  expense: 'edit',
  transfer: 'edit',
  refund: 'none',
  buy: 'edit',
  sell: 'edit',
  convert: 'detail',
  // split 落地（ADR-0106 / #1049）临时形态：改 / 删后端显式拒绝、只读详情未接，
  // 行激活暂为 none（行菜单仅删除项，点击返回码化错误）；#1052 补只读详情后翻转 detail。
  split: 'none',
} satisfies Record<TransactionKind, TransactionKindActivation>

/** 行激活形态查询：行激活与行菜单按 kind 收口的唯一事实源。 */
export function transactionKindActivation(kind: TransactionKind): TransactionKindActivation {
  return TRANSACTION_KIND_ACTIVATION[kind]
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
 * 右键菜单承接，独立 ticket 落地前处于过渡态；不含 convert：无现金腿 kind
 * 界面只读、无手工录入，ADR-0106 决策 10 / #1048）。 */
export const CREATE_KINDS = Object.keys(CREATE_KIND_MAP) as CreateTransactionKind[]

/** 「记一笔」表单形态闭集：可创建 kind + 借贷两个呈现变体（issue #374 / ADR-0053：
 * lend/borrow 不新增交易 kind，落账仍为 transfer + receivable/debt 账户）。 */
export type CreateFormKind = CreateTransactionKind | 'lend' | 'borrow'

/** 「记一笔」借贷变体入口（issue #374）：两项各预设一个方向（反向方向经表单内
 * 方向切换到达），不占快捷键键位。 */
export const LENDING_CREATE_DIRECTIONS = ['lend', 'borrow'] as const

export interface CreateTransactionResult {
  success: boolean
  duplicate: boolean
  id: string | null
  error: string | null
}

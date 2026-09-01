import type { AccountType, CreateFormKind, Transaction, TransactionKind } from '@/types'

/**
 * 借贷方向派生（issue #374 / ADR-0053）：借贷是 transfer + receivable/debt 账户的
 * **派生视角**，不新增交易 kind；方向识别唯一依据是两端账户类型。
 *
 * 本模块是全仓唯一的借贷方向判定点：录入表单（方向预置与切换）、列表类型标签、
 * 编辑形态识别三处消费 `deriveLendingDirection`，不出现第二份方向判定。
 * 纯函数、无 Vue/store 依赖（与 TransactionInput 装配器同一纪律）。
 */

/** 借贷方向五态：借出 / 收回 / 借入 / 还款 / 普通转账（none） */
export type LendingDirection = 'lend' | 'collect' | 'borrow' | 'repay' | 'none'

/** 借贷表单可处的方向（'none' 只在派生/识别时出现，不是表单状态） */
export type LendingFormDirection = Exclude<LendingDirection, 'none'>

/** 账户在借贷语境下的侧别：资金账户 / 借出款（receivable）/ 负债（debt）/ 未知 */
export type LendingAccountSide = 'fund' | 'receivable' | 'debt' | 'unknown'

/** 账户类型 → 借贷侧别：receivable/debt 之外的类型（cash/bank/credit/ewallet/investment/other）都是资金账户 */
export function lendingAccountSide(type: AccountType | null | undefined): LendingAccountSide {
  if (type === 'receivable' || type === 'debt') return type
  return type == null ? 'unknown' : 'fund'
}

/** 账户类型是否属于某侧别（账户类型未知即不属于任何侧） */
export function accountMatchesSide(
  type: AccountType | null | undefined,
  side: LendingAccountSide,
): boolean {
  return lendingAccountSide(type) === side
}

/**
 * 各方向允许的转出/转入账户侧别——借贷表单账户选择器过滤的唯一依据：
 * 借出/收回是「资金账户 ↔ receivable」，借入/还款是「debt ↔ 资金账户」。
 */
export const LENDING_DIRECTION_SIDES: Record<
  LendingFormDirection,
  { from: LendingAccountSide; to: LendingAccountSide }
> = {
  lend: { from: 'fund', to: 'receivable' },
  collect: { from: 'receivable', to: 'fund' },
  borrow: { from: 'debt', to: 'fund' },
  repay: { from: 'fund', to: 'debt' },
}

/** 表单方向闭集（键序即方向切换器的展示序） */
export const LENDING_FORM_DIRECTIONS = Object.keys(LENDING_DIRECTION_SIDES) as LendingFormDirection[]

/**
 * 方向派生矩阵：由 LENDING_DIRECTION_SIDES 镜像生成——「过滤哪侧账户」与「识别什么方向」
 * 同源一处，两表永不漂移（fund→receivable = 借出等四条映射不重复书写）。
 */
const DIRECTION_BY_SIDE_PAIR: Record<
  LendingAccountSide,
  Partial<Record<LendingAccountSide, LendingDirection>>
> = { fund: {}, receivable: {}, debt: {}, unknown: {} }
for (const [direction, sides] of Object.entries(LENDING_DIRECTION_SIDES) as [
  LendingFormDirection,
  { from: LendingAccountSide; to: LendingAccountSide },
][]) {
  DIRECTION_BY_SIDE_PAIR[sides.from][sides.to] = direction
}

/** 方向派生的侧别归一（issue #374 修订）：借贷语义由借贷侧（debt/receivable）唯一决定，
 * 对端类型缺失（黑洞 is_hidden 占位 / 已删 / 不可查）时归一为资金侧参与判定，
 * 不因此退回普通转账。只作用于方向派生；[`lendingAccountSide`] 本身与
 * [`accountMatchesSide`] 的表单过滤语义保持原样（unknown 不命中任何侧）。
 * 两端均非借贷侧时缺失依旧无借贷语义（资金互转、两端缺失 → none）。 */
function directionalSide(type: AccountType | null | undefined): LendingAccountSide {
  const side = lendingAccountSide(type)
  return side === 'unknown' ? 'fund' : side
}

/**
 * 借贷方向派生（全仓唯一点）：kind + 转出账户类型 + 转入账户类型 → 五态。
 * 非 transfer kind、借贷账户互转、资金账户互转 → 普通转账（none）；
 * 借贷侧（debt/receivable）对端类型缺失（黑洞/已删/不可查）时仍按借贷侧方向派生。
 */
export function deriveLendingDirection(
  kind: TransactionKind,
  fromType: AccountType | null | undefined,
  toType: AccountType | null | undefined,
): LendingDirection {
  if (kind !== 'transfer') return 'none'
  return (
    DIRECTION_BY_SIDE_PAIR[directionalSide(fromType)][directionalSide(toType)] ?? 'none'
  )
}

/** 方向 → 文案 key：四方向取借贷文案，普通转账回退转账标签；消费方经 t() 取当前语言 */
export function lendingLabelKey(direction: LendingDirection): string {
  return direction === 'none' ? 'transactions.kind.transfer' : `transactions.lending.${direction}`
}

/**
 * 编辑形态识别（列表标签 / 表单分派 / 借贷表单回填共用）：一笔交易按两端账户类型
 * 解析借贷方向，账户类型经调用方注入的解析器取得（如参考数据映射）。非 transfer、
 * 普通转账或任一端账户类型缺失（账户已删/参考数据未就绪）→ null（按普通转账
 * 呈现，不把未知账户误判成借贷）。
 */
export function resolveLendingDirection(
  tx: Pick<Transaction, 'kind' | 'account_id' | 'to_account_id'>,
  accountType: (id: string) => AccountType | undefined,
): LendingFormDirection | null {
  const direction = deriveLendingDirection(
    tx.kind,
    accountType(tx.account_id),
    tx.to_account_id == null ? undefined : accountType(tx.to_account_id),
  )
  return direction === 'none' ? null : direction
}

/** 「记一笔」表单形态是否为借贷变体入口（lend/borrow，区别于交易 kind） */
export function isLendingEntryKind(kind: CreateFormKind): kind is 'lend' | 'borrow' {
  return kind === 'lend' || kind === 'borrow'
}

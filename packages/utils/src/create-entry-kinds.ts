import { CREATE_KINDS, type CreateTransactionKind } from '@ledger/types'

/**
 * 「记一笔」新建入口的类型可用性（issue #1245 / ADR-0116 决策 4「入口侧」）：
 * 新建买入/卖出是向投资功能写入新数据的入口——关闭投资后它们随入口一并消失，
 * 但既有 buy/sell 交易与引用侧（列表展示、来源列标的链接、按 kind 筛选）一律照常。
 *
 * 单一来源：桌面记一笔下拉、移动「记一笔」悬浮按钮、`b`/`s` 裸键三处入口共用本模块
 * 的判定，不在各入口各写一份「投资关闭要少哪几项」——清单改动只在此处发生。
 */

/** 投资功能关闭后从全部新建入口消失的 kind（入口侧闭集，改清单须同步 ADR-0116 的口径）。 */
export const INVESTMENT_CREATE_KINDS = ['buy', 'sell'] as const satisfies readonly CreateTransactionKind[]

/** 单类型可用性判定：投资关闭 → 买入/卖出不可用；其余 kind 不受影响。 */
export function isCreateKindAvailable(kind: CreateTransactionKind, investmentsClosed: boolean): boolean {
  return !(investmentsClosed && (INVESTMENT_CREATE_KINDS as readonly string[]).includes(kind))
}

/** 可用新建类型（清单序保留）：桌面下拉与移动悬浮按钮渲染同一份结果，一处生效两处。 */
export function availableCreateKinds(investmentsClosed: boolean): CreateTransactionKind[] {
  return CREATE_KINDS.filter((kind) => isCreateKindAvailable(kind, investmentsClosed))
}

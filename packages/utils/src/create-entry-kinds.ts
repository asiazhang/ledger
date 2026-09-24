import {
  CREATE_KINDS,
  type CreateTransactionKind,
  type TransactionPageCreateKind,
} from "@ledger/types";

/**
 * 「记一笔」新建入口的类型可用性判定源（单一来源：桌面记一笔下拉、移动「记一笔」
 * 悬浮按钮、裸键快捷键可用性闸门三处共用，一处生效两处——下拉与 FAB 渲染同一份
 * availableCreateKinds 结果）。
 *
 * ADR-0135 决策 5 / issue #1782：买入/卖出的记一笔入口落投资页「明细」页签头部，
 * 交易页可创建闭集收窄为支出/收入/转账（CREATE_KINDS 单源，归属表见
 * types/create-kind-entry 注释）。收窄是无条件的——关闭投资时投资页整页不可达
 * （ADR-0116 决策 4 修订注记：入口语义由整页覆盖），交易页创建闭集不随功能开关
 * 变化、重开亦不回添买卖；issue #1245 时代「投资关闭 → 买卖退出交易页入口」的
 * 开关闸门随创建入口迁址退役。
 *
 * 既有 buy/sell 交易与引用侧（列表展示、来源列标的链接、按 kind 筛选）一律照常
 * （ADR-0116「入口消失、引用放行」本体不变）。
 */

/** 单类型可用性判定：kind 是否属于交易页创建闭集（CREATE_KINDS 单源）。 */
export function isCreateKindAvailable(kind: CreateTransactionKind): boolean {
  return (CREATE_KINDS as readonly string[]).includes(kind);
}

/** 可用新建类型（清单序保留）：桌面下拉与移动悬浮按钮渲染同一份结果，一处生效两处；
 * 返回元素收窄为交易页入口类型（键位映射 CREATE_KIND_KEYS 的可索引域）。 */
export function availableCreateKinds(): TransactionPageCreateKind[] {
  return CREATE_KINDS.filter(isCreateKindAvailable) as TransactionPageCreateKind[];
}

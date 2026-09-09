import type { Ref } from 'vue'
import { storeToRefs } from 'pinia'
import { useTransactionsSessionStore } from '@/stores/transactions-session'
import type {
  TransactionFilters,
  TransactionFilterPatch,
  TransactionUrlQuery,
} from '@/stores/transactions-session'

/** 过滤常量与类型随深模块状态迁入交易页会话 store（issue #893）；此处再导出维持
 * 既有导入路径（消费方经本模块取用），不制造第二口径。 */
export {
  UNCATEGORIZED_ONLY,
  CATEGORY_DRILLDOWN_KINDS,
  MERCHANT_DRILLDOWN_KINDS,
  TRANSACTION_PAGE_SIZE_DEFAULT,
} from '@/stores/transactions-session'
export type {
  TransactionFilters,
  TransactionFilterPatch,
  TransactionUrlQuery,
} from '@/stores/transactions-session'

export interface UseTransactionFilterReturn {
  /** 过滤状态（只读）：改动只能经意图入口，视图与测试均只读消费。 */
  readonly filters: Readonly<TransactionFilters>
  /** 页码（分页归模块所有）：翻页导航由调用方直写并自行重拉，「翻页归零」只发生在模块出口，
   * 「删除后超页回退」只发生在 afterRowDelete 入口（ADR-0045）。 */
  readonly page: Ref<number>
  /** 页大小（分页归模块所有）：切换后调用方经 refresh() 走统一出口。 */
  readonly pageSize: Ref<number>
  /** 重拉版本号：bump 即「需以当前模块状态重新拉取列表」，是唯一重拉信号。 */
  readonly refreshVersion: Ref<number>
  /** 声明部分过滤意图：任一维度实际变化 → 翻页归零 + 版本 bump（立即生效，无 debounce）。 */
  setFilter(patch: TransactionFilterPatch): void
  /** 清除全部过滤（显式动词）：有可复位状态才动作，回默认态并走统一出口。 */
  resetFilters(): void
  /** 外部数据变化（记一笔/退款回填等）：重拉 + 翻回第一页，不动筛选。 */
  refresh(): void
  /** 页码回退入口（ADR-0045，删除路径专用）：声明「删除当前页一行后本页剩 N 条」——
   * N 为 0 且当前页非第一页时减一页，然后一律版本 bump（保持当前页、不翻回第一页）；
   * 「翻页归零」仍是唯一出口，「回退不归零」是新增显式入口，删除路径不再直写页码。 */
  afterRowDelete(remainingOnPage: number): void
  /** 递入最新路由 query（URL 下钻只读入口，issue #234）：模块按参数表逐维度解析、校验、
   * 复位与就绪补判；URL 只读不写回（#96 决策 3/4），视图只负责监听路由并转发。 */
  syncUrlQuery(query: TransactionUrlQuery): void
}

/**
 * 交易列表过滤深模块（ADR-0030）的对外工厂：「用户意图进、列表状态出」。
 *
 * 工厂形态与对外接口不变（接口收敛为 setFilter / resetFilters / refresh / afterRowDelete /
 * syncUrlQuery 五个意图入口 + 可观察状态 filters、page、pageSize、refreshVersion）。
 * issue #893 起，状态与规则内化迁入交易页会话 store（useTransactionsSessionStore，
 * ADR-0094 会话内保留）：筛选与分页在会话内离开再回来后原样恢复、冷启动回默认、
 * 零写盘；URL 下钻参数永远赢。本工厂退为薄适配：
 * - 声明一次「访次开始」——URL 参数表运行时簿记复位到单次进入边界（URL 在场即
 *   重新消费，覆盖保留态对应维度）；首拉由消费方 setup 期 immediate 读当前模块
 *   状态发起（恢复访次以保留态拉取，不经 refresh 的翻回第一页语义）；
 * - 把 store 的状态投影与意图入口原样交还调用方。
 *
 * 边界（ADR-0030 决策 6）不变：模块只产出请求参数来源（状态）与版本信号（refreshVersion）；
 * 请求发起、loading、行数据仍归调用方。
 */
export function useTransactionFilter(): UseTransactionFilterReturn {
  const store = useTransactionsSessionStore()
  store.beginVisit()
  const { page, pageSize, refreshVersion } = storeToRefs(store)
  return {
    filters: store.filters,
    page,
    pageSize,
    refreshVersion,
    setFilter: store.setFilter,
    resetFilters: store.resetFilters,
    refresh: store.refresh,
    afterRowDelete: store.afterRowDelete,
    syncUrlQuery: store.syncUrlQuery,
  }
}

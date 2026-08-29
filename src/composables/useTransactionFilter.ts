import { reactive, readonly, ref } from 'vue'
import type { Ref } from 'vue'
import type { TransactionKind } from '@/types'

/**
 * 交易列表过滤维度（组件状态是过滤的唯一事实源，URL 仅只读初始化入口、不写回）。
 * 字段与 `TransactionListFilter` 请求参数一一对应（见视图 load 装配）。
 */
export interface TransactionFilters {
  /** 日期起止过滤（YYYY-MM-DD，与后端 date 字典序一致，含边界） */
  dateFrom: string | null
  dateTo: string | null
  /** 涉及账户过滤（account_id 或 to_account_id 命中即算，含转入的转账；issue #97/#98） */
  involvingAccountId: string | null
  /** 商户过滤（含软删商户，历史交易口径；issue #191） */
  merchantId: string | null
  /** 交易类型过滤（前端 6 种：income/expense/transfer/refund/buy/sell） */
  kind: TransactionKind | null
}

/** 部分过滤意图：只声明要改的维度，未提及维度保持不变。 */
export type TransactionFilterPatch = {
  [K in keyof TransactionFilters]?: TransactionFilters[K]
}

export interface UseTransactionFilterReturn {
  /** 过滤状态（只读）：改动只能经意图入口，视图与测试均只读消费。 */
  readonly filters: Readonly<TransactionFilters>
  /** 页码（分页归模块所有）：翻页导航由调用方直写并自行重拉，「翻页归零」只发生在模块出口。 */
  readonly page: Ref<number>
  /** 页大小（分页归模块所有）：切换后调用方经 refresh() 走统一出口。 */
  readonly pageSize: Ref<number>
  /** 重拉版本号：bump 即「需以当前模块状态重新拉取列表」，是唯一重拉信号。 */
  readonly refreshVersion: Ref<number>
  /** 声明部分过滤意图：任一维度实际变化 → 翻页归零 + 版本 bump（立即生效，无 debounce）。 */
  setFilter(patch: TransactionFilterPatch): void
  /** 清除全部过滤（显式动词）：有激活条件才动作，回默认态并走统一出口。 */
  resetFilters(): void
  /** 外部数据变化（记一笔/退款回填等）：重拉 + 翻回第一页，不动筛选。 */
  refresh(): void
}

/**
 * 交易列表过滤深模块（ADR-0030）：「用户意图进、列表状态出」。
 *
 * 工厂形态：每次调用返回独立实例（未来第二个消费者如搜索页需要独立状态，
 * 避免分页与补判串扰）。接口收敛为 setFilter / resetFilters / refresh 三个意图入口
 * + 可观察状态（filters、page、pageSize、refreshVersion），无其他公开面。
 *
 * 统一出口：`apply()` 是全仓唯一「翻页归零 + 刷新」——三个意图入口全部经它生效，
 * 「改筛选必翻页」「回填必翻页」由出口唯一性保证，散布样板与首刷防双刷标志随之消灭
 * （双刷被出口唯一性 + 调用方 watcher 对同步多次 bump 的去重共同消灭）。
 *
 * 边界（ADR-0030 决策 6）：模块只产出请求参数来源（状态）与版本信号（refreshVersion）；
 * 请求发起、loading、行数据仍归调用方。筛选与分页不持久化（ViewState 边界）。
 */
export function useTransactionFilter(): UseTransactionFilterReturn {
  const filters = reactive<TransactionFilters>({
    dateFrom: null,
    dateTo: null,
    involvingAccountId: null,
    merchantId: null,
    kind: null,
  })
  const page = ref(1)
  const pageSize = ref(20)
  const refreshVersion = ref(0)

  /** 统一出口：翻页归零 + 版本 bump ——「翻页归零 + 刷新」全仓仅此一处。 */
  function apply() {
    page.value = 1
    refreshVersion.value += 1
  }

  function setFilter(patch: TransactionFilterPatch) {
    let changed = false
    ;(Object.keys(patch) as Array<keyof TransactionFilters>).forEach((key) => {
      const value = patch[key]
      // undefined 视为未声明（保持 null 不变量）；同值不动作（与既有 setter 守卫语义一致）
      if (value === undefined || filters[key] === value) return
      // 逐键写入：值类型已由 TransactionFilterPatch 的键值对应约束，
      // 此处仅为绕开联合键索引的宽化收窄
      ;(filters as Record<keyof TransactionFilters, unknown>)[key] = value
      changed = true
    })
    // 条件实际变化才触发出口（与既有 setter 的同值守卫语义一致）
    if (changed) apply()
  }

  function resetFilters() {
    // 无激活条件时幂等不动作（清除按钮禁用态的双重保险，保持既有 clearFilters 语义）
    if (!Object.values(filters).some((v) => v !== null)) return
    filters.dateFrom = null
    filters.dateTo = null
    filters.involvingAccountId = null
    filters.merchantId = null
    filters.kind = null
    apply()
  }

  function refresh() {
    apply()
  }

  return {
    filters: readonly(filters),
    page,
    pageSize,
    refreshVersion,
    setFilter,
    resetFilters,
    refresh,
  }
}

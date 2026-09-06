import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api } from '@/api'
import type { Account, Category, Currency, Insurer, Merchant } from '@/types'
import type { Syncable } from '@/types/common'
import {
  rootCategories as pureRootCategories,
  categoryChildren as pureCategoryChildren,
  categoryPath as pureCategoryPath,
  buildCategoryTree as pureBuildCategoryTree,
  type CategoryTreeNode,
} from '@/utils/category-tree'

/** 参考数据加载状态：`idle` 为初始瞬态（self-init 同步置为 `loading`，外部基本观察不到）。 */
export type ReferenceStatus = 'idle' | 'loading' | 'ready' | 'error'

/**
 * 参考数据（Reference Data）单一来源 store。
 *
 * 承载 `currencies / accounts / categories / merchants` 四张参考表、保司字典
 * `insurers`（保险域自有，ADR-0082，issue #714）及全部派生映射
 * （账户/分类/币种/商户映射）与分类树逻辑，作为字典/枚举的单一来源，消费端一律从本 store 读取。
 *
 * 生命周期（push-first）：
 * - 首次访问 self-init：store 首次被创建时自动触发一次加载；
 * - 订阅后端 `ledger:changed`：参考写入成功后自动重拉四表
 *   （stale-while-revalidate：拉取期间保留旧数据，全部成功才整体替换，不闪空）；
 * - 派生映射为 computed，随数组自动更新。
 *
 * 失效机制唯一：事件驱动（`ledger:changed`）是唯一的重拉触发源，无 pull 侧兜底。
 * 失效信号：`status`（idle/loading/ready/error）与 `version`（每次成功重拉自增），
 * 供观测加载状态与重拉次数。动作：`refresh()`（强制重拉，在途合并去重）。
 */
/**
 * 含删全量字典行的统一拆分（reload 内分类/商户/保司三处同构）：
 * 在用行进字典，软删行进显示/管理视图缓存 Map（历史引用与「显示已删」共用）。
 */
function splitDeleted<T extends Syncable & { id: string }>(all: T[]): { active: T[]; deleted: Map<string, T> } {
  return {
    active: all.filter((r) => !r.is_deleted),
    deleted: new Map(all.filter((r) => r.is_deleted).map((r) => [r.id, r])),
  }
}

export const useReferenceStore = defineStore('reference', () => {
  const currencies = ref<Currency[]>([])
  const accounts = ref<Account[]>([])
  const categories = ref<Category[]>([])
  const merchants = ref<Merchant[]>([])
  const insurers = ref<Insurer[]>([])

  /**
   * 软删商户（issue #189 / ADR-0028）：软删后不可再被选择，但历史交易引用照常显示。
   * 数据源为后端含软删全量列表（`list_merchants({ includeDeleted: true })`，issue #191）
   * 按 `is_deleted` 拆分而来：跨会话可用，无需 diff 缓存。
   * 商户管理列表「显示已删」（issue #447）消费同一份缓存，无新增拉取。
   */
  const deletedMerchants = ref(new Map<string, Merchant>())

  /**
   * 软删分类（issue #377）：软删后不可再被选择，但历史交易引用照常存在。
   * 数据源与拆分方式同商户先例（issue #191）：含软删全量列表按 `is_deleted` 拆分。
   */
  const deletedCategories = ref(new Map<string, Category>())

  /**
   * 保司字典（issue #714 / ADR-0082）：保险域自有独立字典，虽不归参考数据域，
   * 但同享「随 ledger:changed 失效自动重拉」的参考数据心智，接入本 store
   * （与 #713 的保单换轨同源消费；管理视图「显示已删」切换消费软删缓存）。
   * 数据源与拆分方式同商户先例（issue #191）：含已删全量列表按 `is_deleted` 拆分。
   */
  const deletedInsurers = ref(new Map<string, Insurer>())

  // —— 失效信号 ——
  const status = ref<ReferenceStatus>('idle')
  const version = ref(0)

  /** 在途加载 promise（并发调用合并去重）。 */
  let inFlight: Promise<void> | null = null

  const currencyMap = computed(() => {
    const m = new Map<string, Currency>()
    currencies.value.forEach((c) => m.set(c.code, c))
    return m
  })

  /** 分类显示/下钻校验映射：在用 + 软删（历史交易口径，URL 下钻校验共用，issue #377；先例商户）。 */
  const categoryMap = computed(() => {
    const m = new Map<string, Category>()
    deletedCategories.value.forEach((d) => m.set(d.id, d))
    categories.value.forEach((c) => m.set(c.id, c))
    return m
  })

  const accountMap = computed(() => {
    const m = new Map<string, Account>()
    accounts.value.forEach((a) => m.set(a.id, a))
    return m
  })

  /** 商户显示映射：在用商户 + 软删商户（历史交易显示与筛选下拉共用，issue #191）。 */
  const merchantMap = computed(() => {
    const m = new Map<string, Merchant>()
    deletedMerchants.value.forEach((d) => m.set(d.id, d))
    merchants.value.forEach((m2) => m.set(m2.id, m2))
    return m
  })

  /** 按名字查找：仅含在用商户（软删商户不可再选/不可按名复用，重名即建由后端校验）。 */
  const merchantByName = computed(() => {
    const m = new Map<string, Merchant>()
    merchants.value.forEach((m2) => m.set(m2.name, m2))
    return m
  })

  const rootCategories = computed(() => pureRootCategories(categories.value))

  const expenseCategories = computed(() =>
    categories.value.filter((c) => c.kind === 'expense'),
  )
  const incomeCategories = computed(() =>
    categories.value.filter((c) => c.kind === 'income'),
  )

  function categoryChildren(parentId: string): Category[] {
    return pureCategoryChildren(categories.value, parentId)
  }

  function categoryPath(id: string | null | undefined): string {
    return pureCategoryPath(categories.value, id)
  }

  /** 分类显示名（issue #356）：顶级为自身名，子分类为「父 > 子」路径名；
   * id 解析不到（孤儿引用，如守卫生效前的历史预算）时回退调用方提供的
   * 后端兜底名（「未分类」），不抛错。 */
  function categoryDisplayName(id: string | null | undefined, fallback: string): string {
    return categoryPath(id) || fallback
  }

  function treeCategoryOptions(kind: Category['kind']): CategoryTreeNode[] {
    return pureBuildCategoryTree(categories.value, { kind })
  }

  /**
   * 核心：一次完整重拉，stale-while-revalidate。
   * 拉取期间保留旧数据；四表全部成功才整体替换（避免闪空与部分更新）。
   */
  async function reload(): Promise<void> {
    status.value = 'loading'
    try {
      const [cs, as, catsAll, msAll, insAll] = await Promise.all([
        api.listCurrencies(),
        api.listAccounts(),
        // 分类拉含软删全量，按 is_deleted 拆分：在用进字典，软删进显示/校验缓存
        // （历史交易口径，issue #377，先例商户 issue #191）
        api.listCategories({ includeDeleted: true }),
        // 商户拉含软删全量，按 is_deleted 拆分：在用进字典，软删进显示缓存（issue #191）
        api.listMerchants({ includeDeleted: true }),
        // 保司拉含已删全量，按 is_deleted 拆分：在用进字典，已删进管理视图显示缓存（issue #714）
        api.listInsurers({ includeDeleted: true }),
      ])
      currencies.value = cs
      accounts.value = as
      const cats = splitDeleted(catsAll)
      categories.value = cats.active
      deletedCategories.value = cats.deleted
      const ms = splitDeleted(msAll)
      merchants.value = ms.active
      deletedMerchants.value = ms.deleted
      const ins = splitDeleted(insAll)
      insurers.value = ins.active
      deletedInsurers.value = ins.deleted
      version.value += 1
      status.value = 'ready'
    } catch (e) {
      status.value = 'error'
      throw e
    }
  }

  /** 在途去重：并发调用（self-init / refresh / 事件）合并为同一次加载。 */
  function reloadMerged(): Promise<void> {
    if (inFlight) return inFlight
    inFlight = reload().finally(() => {
      inFlight = null
    })
    return inFlight
  }

  /** 强制重拉（在途时合并，避免 IPC 风暴）。 */
  function refresh(): Promise<void> {
    return reloadMerged()
  }

  // —— push 生命周期 ——
  // 首次访问 self-init：触发一次加载（失败静默，失败信号已由 status 承载）。
  void refresh().catch(() => {
    /* noop */
  })

  // 订阅后端 ledger:changed：参考数据已失效 → 静默重拉（stale-while-revalidate）。
  // 注：注册为异步；注册完成前到达的事件会丢失（窗口极窄：应用启动瞬间，
  // AI 导入写入几乎不会恰好发生在该时刻；后续任一 refresh/事件仍会兜底）。
  listen('ledger:changed', () => {
    void refresh().catch(() => {
      /* noop：失败信号已由 status 承载 */
    })
  }).catch(() => {
    /* 监听注册失败不阻塞 store（本地事件，极少发生） */
  })

  function getCurrency(code: string): Currency | undefined {
    return currencyMap.value.get(code)
  }

  return {
    currencies,
    accounts,
    categories,
    merchants,
    insurers,
    deletedMerchants,
    deletedInsurers,
    status,
    version,
    currencyMap,
    categoryMap,
    accountMap,
    merchantMap,
    merchantByName,
    rootCategories,
    expenseCategories,
    incomeCategories,
    categoryChildren,
    categoryPath,
    categoryDisplayName,
    treeCategoryOptions,
    refresh,
    getCurrency,
  }
})

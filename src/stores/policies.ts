import { defineStore } from 'pinia'
import { computed, ref } from 'vue'
import { listen } from '@tauri-apps/api/event'
import { api } from '@/api'
import type { Policy, PolicyInput, PolicyStats } from '@/types'

/** 保单加载状态：`idle` 为初始瞬态（self-init 同步置为 `loading`，外部基本观察不到）。 */
export type PoliciesStatus = 'idle' | 'loading' | 'ready' | 'error'

/**
 * 保单（Policy）领域 store（issue #360 / ADR-0051）。
 *
 * 保单是消费型保险合同的静态档案（保险分域词汇表 `Policy`），**不进**
 * `useReferenceStore`（不是可选值字典），拥有自己的单一来源 store
 * （镜像 `useItemsStore` 的 push-first 生命周期）：
 * - 首次访问 self-init：store 首次被创建时自动触发一次加载；
 * - 订阅后端 `ledger:changed`：保单写入（本 store 或后续写路径）成功后自动重拉
 *   （stale-while-revalidate：拉取期间保留旧数据，成功后整体替换，不闪空）。
 *
 * 列表只含未删除保单（软删不进列表）；到期状态不在此持久化或推导，
 * 展示层由保障期间即时推导（可推导的状态不落库）。
 *
 * 逐保单视角统计（issue #363）：与列表同生命周期重拉（同一批 IPC 写入都触发
 * `ledger:changed`），`statsById` 供视图按保单 id 取统计行——实时推导不落库，
 * 前端不做二次聚合。
 */
export const usePoliciesStore = defineStore('policies', () => {
  const policies = ref<Policy[]>([])
  const stats = ref<PolicyStats[]>([])
  const status = ref<PoliciesStatus>('idle')
  const version = ref(0)

  /** 保单 id → 统计行（视图按行取统计的单点查法）。 */
  const statsById = computed(() => new Map(stats.value.map((s) => [s.policy_id, s])))

  /** 在途加载 promise（并发调用合并去重）。 */
  let inFlight: Promise<void> | null = null

  /** 一次完整重拉：列表与统计同批刷新，拉取期间保留旧数据，成功后整体替换
   * （避免闪空与部分更新；任一失败则整体失败，失败信号由 status 承载）。 */
  async function reload(): Promise<void> {
    status.value = 'loading'
    try {
      const [list, statsList] = await Promise.all([api.listPolicies(), api.listPolicyStats()])
      policies.value = list
      stats.value = statsList
      version.value += 1
      status.value = 'ready'
    } catch (e) {
      status.value = 'error'
      throw e
    }
  }

  /** 在途去重：并发调用（self-init / refresh / create / 事件）合并为同一次加载。 */
  function reloadMerged(): Promise<void> {
    if (inFlight) return inFlight
    inFlight = reload().finally(() => {
      inFlight = null
    })
    return inFlight
  }

  /** 强制刷新（在途时合并，避免 IPC 风暴）。 */
  function refresh(): Promise<void> {
    return reloadMerged()
  }

  /** 创建保单：成功后立即重拉（后端同时发 ledger:changed，事件侧重拉被在途合并）。 */
  async function create(input: PolicyInput): Promise<string> {
    const id = await api.createPolicy(input)
    await refresh()
    return id
  }

  /** 按 id 编辑保单静态要素：成功后立即重拉（同 create）。 */
  async function update(id: string, input: PolicyInput): Promise<void> {
    await api.updatePolicy(id, input)
    await refresh()
  }

  /** 软删除保单（后端打 is_deleted=1，不物理移除、引用不置空）：成功后立即重拉。 */
  async function remove(id: string): Promise<void> {
    await api.deletePolicy(id)
    await refresh()
  }

  // —— push 生命周期 ——
  // 首次访问 self-init：触发一次加载（失败静默，失败信号已由 status 承载）。
  void refresh().catch(() => {
    /* noop */
  })

  // 订阅后端 ledger:changed：保单写入即失效 → 静默重拉（stale-while-revalidate）。
  listen('ledger:changed', () => {
    void refresh().catch(() => {
      /* noop：失败信号已由 status 承载 */
    })
  }).catch(() => {
    /* 监听注册失败不阻塞 store（本地事件，极少发生） */
  })

  return {
    policies,
    stats,
    statsById,
    status,
    version,
    refresh,
    create,
    update,
    remove,
  }
})
